//! End-to-end tests against a local mock upstream. No live provider calls.

use std::{
    convert::Infallible,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use bytes::Bytes;
use deslag_core::BoundedExecutor;
use deslag_proxy::{Limits, Upstream, serve, serve_with};
use http_body_util::{BodyExt, Full, channel::Channel, combinators::BoxBody};
use hyper::{
    HeaderMap, Request, Response, body::Incoming, server::conn::http1, service::service_fn,
};
use hyper_util::rt::TokioIo;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    time::timeout,
};

type MockBody = BoxBody<Bytes, Infallible>;
const WAIT: Duration = Duration::from_secs(5);

struct Seen {
    method: String,
    uri: String,
    headers: HeaderMap,
    body: Bytes,
}

/// Mock upstream: records every request and answers with `respond()`.
async fn mock<F>(respond: F) -> (SocketAddr, mpsc::UnboundedReceiver<Seen>)
where
    F: Fn() -> Response<MockBody> + Clone + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let (tx, respond) = (tx.clone(), respond.clone());
            let svc = service_fn(move |req: Request<Incoming>| {
                let (tx, respond) = (tx.clone(), respond.clone());
                async move {
                    let (parts, body) = req.into_parts();
                    let body = body.collect().await.unwrap().to_bytes();
                    let _ = tx.send(Seen {
                        method: parts.method.to_string(),
                        uri: parts.uri.to_string(),
                        headers: parts.headers,
                        body,
                    });
                    Ok::<_, Infallible>(respond())
                }
            });
            tokio::spawn(http1::Builder::new().serve_connection(TokioIo::new(stream), svc));
        }
    });
    (addr, rx)
}

fn full(status: u16, headers: &[(&str, &str)], body: impl Into<Bytes>) -> Response<MockBody> {
    let mut resp = Response::builder().status(status);
    for (k, v) in headers {
        resp = resp.header(*k, *v);
    }
    resp.body(Full::new(body.into()).boxed()).unwrap()
}

/// Mock upstream whose single response body is fed by the returned sender.
async fn streaming_mock() -> (
    SocketAddr,
    mpsc::UnboundedReceiver<Seen>,
    http_body_util::channel::Sender<Bytes>,
) {
    let (sender, body) = Channel::<Bytes>::new(1);
    let body = Arc::new(Mutex::new(Some(body)));
    let (addr, seen) = mock(move || {
        let body = body.lock().unwrap().take().expect("one streaming request");
        Response::builder()
            .header("content-type", "text/event-stream")
            .body(body.boxed())
            .unwrap()
    })
    .await;
    (addr, seen, sender)
}

async fn proxy_to(upstream: SocketAddr) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let upstream = Upstream::parse(&format!("http://{upstream}")).unwrap();
    tokio::spawn(serve(listener, upstream));
    addr
}

async fn send(proxy: SocketAddr, req: Request<Full<Bytes>>) -> Response<Incoming> {
    let stream = TcpStream::connect(proxy).await.unwrap();
    let (mut tx, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(conn);
    tx.send_request(req).await.unwrap()
}

async fn body_bytes(resp: Response<Incoming>) -> Bytes {
    timeout(WAIT, resp.into_body().collect())
        .await
        .unwrap()
        .unwrap()
        .to_bytes()
}

#[tokio::test]
async fn relays_request_and_response_faithfully() {
    let (upstream, mut seen) = mock(|| {
        full(
            200,
            &[
                ("content-type", "application/json"),
                ("request-id", "req_1"),
                ("anthropic-ratelimit-requests-remaining", "9"),
                ("connection", "keep-alive, x-upstream-hop"),
                ("x-upstream-hop", "1"),
                ("keep-alive", "timeout=5"),
            ],
            r#"{"id":"msg_1", "unknown":true}"#,
        )
    })
    .await;
    let proxy = proxy_to(upstream).await;

    let req_body = r#"{ "model": "m", "messages": [], "future_field": [1, 2] }"#;
    let resp = send(
        proxy,
        Request::post("/v1/messages?beta=true")
            .header("host", "client.invalid")
            .header("x-api-key", "sk-test")
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "a-2025,b-2026")
            .header("content-type", "application/json")
            .header("connection", "keep-alive, x-hop")
            .header("x-hop", "1")
            .header("proxy-authorization", "Basic cHJveHk=")
            .body(Full::new(Bytes::from_static(req_body.as_bytes())))
            .unwrap(),
    )
    .await;

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["request-id"], "req_1");
    assert_eq!(
        resp.headers()["anthropic-ratelimit-requests-remaining"],
        "9"
    );
    assert!(!resp.headers().contains_key("x-upstream-hop"));
    assert!(!resp.headers().contains_key("keep-alive"));
    assert_eq!(
        body_bytes(resp).await,
        r#"{"id":"msg_1", "unknown":true}"#.as_bytes()
    );

    let s = seen.recv().await.unwrap();
    assert_eq!(s.method, "POST");
    assert_eq!(s.uri, "/v1/messages?beta=true");
    assert_eq!(s.body, req_body.as_bytes());
    assert_eq!(s.headers["host"], upstream.to_string());
    assert_eq!(s.headers["x-api-key"], "sk-test");
    assert_eq!(s.headers["anthropic-version"], "2023-06-01");
    assert_eq!(s.headers["anthropic-beta"], "a-2025,b-2026");
    assert_eq!(s.headers["content-type"], "application/json");
    assert!(!s.headers.contains_key("x-hop"));
    assert!(!s.headers.contains_key("proxy-authorization"));
}

#[tokio::test]
async fn relays_provider_errors_and_rate_limits() {
    for (status, kind) in [
        (400, "invalid_request_error"),
        (429, "rate_limit_error"),
        (529, "overloaded_error"),
    ] {
        let body = format!(r#"{{"type":"error","error":{{"type":"{kind}","message":"x"}}}}"#);
        let expected = body.clone();
        let (upstream, _seen) = mock(move || {
            full(
                status,
                &[
                    ("retry-after", "7"),
                    ("x-should-retry", "true"),
                    ("anthropic-ratelimit-tokens-reset", "2026-10-11T00:00:00Z"),
                ],
                body.clone(),
            )
        })
        .await;
        let proxy = proxy_to(upstream).await;
        let resp = send(
            proxy,
            Request::post("/v1/messages")
                .body(Full::new(Bytes::from_static(b"{}")))
                .unwrap(),
        )
        .await;
        assert_eq!(resp.status(), status);
        assert_eq!(resp.headers()["retry-after"], "7");
        assert_eq!(resp.headers()["x-should-retry"], "true");
        assert_eq!(
            resp.headers()["anthropic-ratelimit-tokens-reset"],
            "2026-10-11T00:00:00Z"
        );
        assert_eq!(body_bytes(resp).await, expected.as_bytes());
    }
}

#[tokio::test]
async fn streams_sse_without_buffering() {
    let (upstream, _seen, mut tx) = streaming_mock().await;
    let proxy = proxy_to(upstream).await;
    let resp = send(
        proxy,
        Request::post("/v1/messages")
            .body(Full::new(Bytes::from_static(br#"{"stream":true}"#)))
            .unwrap(),
    )
    .await;
    assert_eq!(resp.headers()["content-type"], "text/event-stream");
    let mut body = resp.into_body();

    // A fragment that ends mid-frame must reach the client while upstream is still open.
    let first = "event: message_start\ndata: {\"type\":\"mess";
    tx.send_data(Bytes::from_static(first.as_bytes()))
        .await
        .unwrap();
    let mut got = Vec::new();
    while got.len() < first.len() {
        let frame = timeout(WAIT, body.frame()).await.unwrap().unwrap().unwrap();
        got.extend_from_slice(&frame.into_data().unwrap());
    }
    assert_eq!(got, first.as_bytes());

    let mut expected = first.to_string();
    let mut chunks = vec![
        "age_start\"}\n\n".to_string(),
        "event: ping\ndata: {\"type\": \"ping\"}\n\n".to_string(),
        "data: line one\ndata: line two\n\n".to_string(),
        "event: future_event\ndata: {\"unknown\":1}\n\n".to_string(),
        "event: message_delta\ndata: {\"usage\":{\"output_tokens\":5000}}\n\n".to_string(),
    ];
    for i in 0..5000 {
        chunks.push(format!(
            "event: content_block_delta\ndata: {{\"index\":0,\"delta\":{{\"text\":\"t{i}\"}}}}\n\n"
        ));
    }
    chunks.push("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string());
    expected.push_str(&chunks.concat());
    let sender = tokio::spawn(async move {
        for chunk in chunks {
            // Arbitrary network boundaries: split every chunk in two.
            let (a, b) = chunk.split_at(chunk.len() / 2);
            tx.send_data(Bytes::copy_from_slice(a.as_bytes()))
                .await
                .unwrap();
            tx.send_data(Bytes::copy_from_slice(b.as_bytes()))
                .await
                .unwrap();
        }
    });
    let rest = timeout(WAIT, body.collect())
        .await
        .unwrap()
        .unwrap()
        .to_bytes();
    got.extend_from_slice(&rest);
    sender.await.unwrap();
    assert_eq!(String::from_utf8(got).unwrap(), expected);
}

#[tokio::test]
async fn streams_sse_with_mutation_enabled() {
    use deslag_proxy::session::{Mode, Owner, Store, write_owner};
    let (upstream, mut seen, mut tx) = streaming_mock().await;
    let dir = std::env::temp_dir().join(format!("deslag-sse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    write_owner(&dir, Owner::Proxy).unwrap();
    let store = Store {
        dir: dir.clone(),
        mode: Mode::Safe,
    };
    // The session starts before its first tool result.
    store.replay("sse", "m", &[], |_| true).unwrap();
    // No test reports to a real state dir. SAFETY: only std reads the
    // environment here, and std serializes that access.
    unsafe { std::env::set_var("DESLAG_STATE_DIR", "/nonexistent/deslag-test") };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy = listener.local_addr().unwrap();
    let upstream = Upstream::parse(&format!("http://{upstream}"))
        .unwrap()
        .with_json_minification(store);
    // A budget no debug build on a slow runner can miss, as in minify_json.rs.
    let limits = Limits {
        executor: BoundedExecutor::new(4, Duration::from_secs(60)).unwrap(),
        ..Limits::default()
    };
    tokio::spawn(serve_with(listener, upstream, limits));

    let body = r#"{"model":"m","stream":true,"messages":[
{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]},
{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"[ 1 ]"}]}]}"#;
    let resp = send(
        proxy,
        Request::post("/v1/messages")
            .header("x-claude-code-session-id", "sse")
            .body(Full::new(Bytes::from_static(body.as_bytes())))
            .unwrap(),
    )
    .await;
    assert_eq!(resp.headers()["content-type"], "text/event-stream");
    let up = seen.recv().await.unwrap();
    assert_eq!(up.body, body.replace(r#""[ 1 ]""#, r#""[1]""#).as_bytes());
    let mut body = resp.into_body();

    // Each fragment reaches the client while upstream is still open.
    for fragment in [
        "event: message_start\ndata: {\"ty",
        "pe\":\"message_start\"}\n\n",
    ] {
        tx.send_data(Bytes::from_static(fragment.as_bytes()))
            .await
            .unwrap();
        let mut got = Vec::new();
        while got.len() < fragment.len() {
            let frame = timeout(WAIT, body.frame()).await.unwrap().unwrap().unwrap();
            got.extend_from_slice(&frame.into_data().unwrap());
        }
        assert_eq!(got, fragment.as_bytes());
    }
    drop(tx);
    let rest = timeout(WAIT, body.collect()).await.unwrap().unwrap();
    assert!(rest.to_bytes().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn client_disconnect_cancels_upstream() {
    let (upstream, _seen, mut tx) = streaming_mock().await;
    let proxy = proxy_to(upstream).await;

    let mut client = TcpStream::connect(proxy).await.unwrap();
    client
        .write_all(b"POST /v1/messages HTTP/1.1\r\nhost: x\r\ncontent-length: 2\r\n\r\n{}")
        .await
        .unwrap();
    tx.send_data(Bytes::from_static(b"event: message_start\ndata: {}\n\n"))
        .await
        .unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !String::from_utf8_lossy(&buf).contains("message_start") {
        let n = timeout(WAIT, client.read(&mut chunk))
            .await
            .unwrap()
            .unwrap();
        assert!(n > 0, "proxy closed before first event");
        buf.extend_from_slice(&chunk[..n]);
    }
    drop(client);

    // Upstream keeps streaming; once the proxy drops its upstream body, sends fail.
    timeout(WAIT, async {
        while tx
            .send_data(Bytes::from_static(b"event: ping\ndata: {}\n\n"))
            .await
            .is_ok()
        {}
    })
    .await
    .expect("upstream body was not released after client disconnect");
}

#[tokio::test]
async fn only_configured_upstream_is_contacted() {
    let (upstream, mut seen) = mock(|| full(200, &[], "ok")).await;
    let (decoy, mut decoy_seen) = mock(|| full(200, &[], "decoy")).await;
    let proxy = proxy_to(upstream).await;

    // Absolute-form target and Host header both name the decoy.
    let mut client = TcpStream::connect(proxy).await.unwrap();
    let req = format!(
        "GET http://{decoy}/v1/models HTTP/1.1\r\nhost: {decoy}\r\nx-api-key: sk-secret\r\nconnection: close\r\n\r\n"
    );
    client.write_all(req.as_bytes()).await.unwrap();
    let mut resp = String::new();
    timeout(WAIT, client.read_to_string(&mut resp))
        .await
        .unwrap()
        .unwrap();
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    assert!(resp.ends_with("ok"), "{resp}");

    let s = seen.recv().await.unwrap();
    assert_eq!(s.uri, "/v1/models");
    assert_eq!(s.headers["host"], upstream.to_string());
    assert_eq!(s.headers["x-api-key"], "sk-secret");
    assert!(
        timeout(Duration::from_millis(200), decoy_seen.recv())
            .await
            .is_err(),
        "decoy upstream was contacted"
    );
}

#[tokio::test]
async fn upstream_failure_is_502_without_secrets() {
    // Holds the port and closes every connection before any response.
    let dead_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead = dead_listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = dead_listener.accept().await {
            drop(stream);
        }
    });
    let proxy = proxy_to(dead).await;
    let resp = send(
        proxy,
        Request::post("/v1/messages")
            .header("x-api-key", "sk-secret-123")
            .header("authorization", "Bearer sk-secret-456")
            .body(Full::new(Bytes::from_static(b"{}")))
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), 502);
    let body = String::from_utf8(body_bytes(resp).await.to_vec()).unwrap();
    assert!(body.contains("\"type\":\"error\""), "{body}");
    assert!(!body.contains("sk-secret"), "{body}");
}
