//! Opt-in JSON minification of Bash tool results, against a local mock upstream.

use std::{convert::Infallible, net::SocketAddr, time::Duration};

use bytes::Bytes;
use deslag_core::BoundedExecutor;
use deslag_proxy::{Limits, Upstream, serve_with};
use http_body_util::{BodyExt, Full, channel::Channel};
use hyper::{
    HeaderMap, Request, Response, body::Incoming, server::conn::http1, service::service_fn,
};
use hyper_util::rt::TokioIo;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
};

/// Decoded tool output and its minified form.
const PRETTY: &str = "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": \"x  y\"\n}\n";
const MIN: &str = r#"{"a":[1,2],"b":"x  y"}"#;

/// `{BASH}` marks every lexeme the proxy must minify; everything else must stay byte-identical.
const TURN_1: &str = r#"{
  "model": "m",
  "system": {PRETTY},
  "tools": [ { "name": "Bash", "input_schema": { "type" : "object" } } ],
  "messages": [
    { "role": "user", "content": {PRETTY} },
    { "role": "assistant", "content": [
      { "type": "text", "text": {PRETTY} },
      { "type": "tool_use", "id": "toolu_bash", "name": "Bash", "input": { "command": {PRETTY} } },
      { "type": "tool_use", "id": "toolu_read", "name": "Read", "input": { "file_path": "/x.json" } },
      { "type": "tool_use", "id": "toolu_bash_blocks", "name": "Bash", "input": { } }
    ] },
    { "role": "user", "content": [
      { "type": "tool_result", "tool_use_id": "toolu_bash", "content": {BASH}, "is_error": false },
      { "type": "tool_result", "tool_use_id": "toolu_read", "content": {PRETTY} },
      { "type": "tool_result", "tool_use_id": "toolu_bash_blocks", "content": [
        { "type": "text", "text": {BASH} },
        { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AA==" } },
        { "type": "text", "text": "plain  text" }
      ], "cache_control": { "type": "ephemeral" } }
    ] }
  ]
}"#;

/// Turn 2 resends turn 1's history, moves `cache_control` to the new result and appends a turn.
const TURN_2: &str = r#"{
  "model": "m",
  "system": {PRETTY},
  "tools": [ { "name": "Bash", "input_schema": { "type" : "object" } } ],
  "messages": [
    { "role": "user", "content": {PRETTY} },
    { "role": "assistant", "content": [
      { "type": "text", "text": {PRETTY} },
      { "type": "tool_use", "id": "toolu_bash", "name": "Bash", "input": { "command": {PRETTY} } },
      { "type": "tool_use", "id": "toolu_read", "name": "Read", "input": { "file_path": "/x.json" } },
      { "type": "tool_use", "id": "toolu_bash_blocks", "name": "Bash", "input": { } }
    ] },
    { "role": "user", "content": [
      { "type": "tool_result", "tool_use_id": "toolu_bash", "content": {BASH}, "is_error": false },
      { "type": "tool_result", "tool_use_id": "toolu_read", "content": {PRETTY} },
      { "type": "tool_result", "tool_use_id": "toolu_bash_blocks", "content": [
        { "type": "text", "text": {BASH} },
        { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "AA==" } },
        { "type": "text", "text": "plain  text" }
      ] }
    ] },
    { "role": "assistant", "content": [
      { "type": "tool_use", "id": "toolu_bash_2", "name": "Bash", "input": { "command": "ls" } }
    ] },
    { "role": "user", "content": [
      { "type": "tool_result", "tool_use_id": "toolu_bash_2", "content": {BASH}, "cache_control": { "type": "ephemeral" } }
    ] }
  ]
}"#;

fn lit(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

/// The client request and the body the upstream must receive.
fn render(template: &str) -> (String, String) {
    let base = template.replace("{PRETTY}", &lit(PRETTY));
    (
        base.replace("{BASH}", &lit(PRETTY)),
        base.replace("{BASH}", &lit(MIN)),
    )
}

struct Seen {
    headers: HeaderMap,
    body: Bytes,
}

async fn mock() -> (SocketAddr, mpsc::UnboundedReceiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let tx = tx.clone();
            let svc = service_fn(move |req: Request<Incoming>| {
                let tx = tx.clone();
                async move {
                    let (parts, body) = req.into_parts();
                    let body = body.collect().await.unwrap().to_bytes();
                    let _ = tx.send(Seen {
                        headers: parts.headers,
                        body,
                    });
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"{}"))))
                }
            });
            tokio::spawn(http1::Builder::new().serve_connection(TokioIo::new(stream), svc));
        }
    });
    (addr, rx)
}

/// A budget no debug build on a slow runner can miss, so tests that expect a
/// rewrite never see a fail-open rejection.
async fn proxy(minify: bool) -> (SocketAddr, mpsc::UnboundedReceiver<Seen>) {
    proxy_with(
        minify,
        BoundedExecutor::new(4, Duration::from_secs(60)).unwrap(),
    )
    .await
}

async fn proxy_with(
    minify: bool,
    executor: BoundedExecutor,
) -> (SocketAddr, mpsc::UnboundedReceiver<Seen>) {
    let (upstream, seen) = mock().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut upstream = Upstream::parse(&format!("http://{upstream}")).unwrap();
    if minify {
        upstream = upstream.with_json_minification();
    }
    let limits = Limits {
        executor,
        ..Limits::default()
    };
    // No service lives here, so no test reports to a real state dir. SAFETY:
    // only std reads the environment here, and std serializes that access.
    unsafe { std::env::set_var("DESLAG_STATE_DIR", "/nonexistent/deslag-test") };
    tokio::spawn(serve_with(listener, upstream, limits));
    (addr, seen)
}

/// Sends one request through the proxy and returns what the upstream received.
async fn relay(
    (proxy, seen): &mut (SocketAddr, mpsc::UnboundedReceiver<Seen>),
    req: hyper::http::request::Builder,
    body: impl Into<Bytes>,
) -> Seen {
    let stream = TcpStream::connect(*proxy).await.unwrap();
    let (mut tx, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(conn);
    let resp = tx
        .send_request(req.body(Full::new(body.into())).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let seen = seen.recv().await.unwrap();
    // hyper omits content-length for an empty body.
    let length = seen
        .headers
        .get("content-length")
        .map(|v| v.to_str().unwrap());
    let expected = (!seen.body.is_empty()).then(|| seen.body.len().to_string());
    assert_eq!(
        length,
        expected.as_deref(),
        "content-length must match the body"
    );
    seen
}

fn messages() -> hyper::http::request::Builder {
    Request::post("/v1/messages?beta=true").header("content-type", "application/json")
}

#[tokio::test]
async fn minifies_only_bash_tool_results() {
    let (input, expected) = render(TURN_1);
    assert_ne!(input, expected);
    let mut p = proxy(true).await;
    let seen = relay(&mut p, messages(), input).await;
    assert_eq!(String::from_utf8(seen.body.to_vec()).unwrap(), expected);
}

#[tokio::test]
async fn history_does_not_drift_on_resubmission() {
    let mut p = proxy(true).await;
    let (input_1, expected_1) = render(TURN_1);
    let (input_2, expected_2) = render(TURN_2);
    let up_1 = relay(&mut p, messages(), input_1).await.body;
    let up_2 = relay(&mut p, messages(), input_2.clone()).await.body;
    assert_eq!(up_1, expected_1.as_bytes());
    assert_eq!(up_2, expected_2.as_bytes());
    // Everything before the moved cache_control is the same bytes in both turns.
    let cut = expected_1.find("], \"cache_control\"").unwrap();
    assert_eq!(up_2[..cut], up_1[..cut]);
    let again = relay(&mut p, messages(), input_2).await.body;
    assert_eq!(again, up_2);
}

#[tokio::test]
async fn ineligible_requests_are_byte_identical() {
    let (input, _) = render(TURN_1);
    let mut off = proxy(false).await;
    assert_eq!(relay(&mut off, messages(), input.clone()).await.body, input);

    let mut on = proxy(true).await;
    for req in [
        Request::post("/v1/messages/count_tokens"),
        Request::post("/v1/complete"),
        Request::put("/v1/messages"),
        messages().header("content-encoding", "gzip"),
    ] {
        assert_eq!(relay(&mut on, req, input.clone()).await.body, input);
    }
    for body in ["not json", "[]", r#"{"messages": "x"}"#, ""] {
        assert_eq!(relay(&mut on, messages(), body).await.body, body);
    }
}

#[tokio::test]
async fn blocks_with_duplicate_or_escaped_keys_are_skipped() {
    let template = r#"{"messages": [
  {"role": "assistant", "content": [
    {"type": "tool_use", "id": "a", "name": "Bash", "input": {}},
    {"type": "tool_use", "id": "b", "name": "Bash", "input": {}},
    {"type": "tool_use", "id": "c", "name": "Bash", "input": {}}
  ]},
  {"role": "user", "content": [
    {"type": "tool_result", "tool_use_id": "a", "content": {PRETTY}, "content": {PRETTY}},
    {"type": "tool_result", "tool_use_id": "b", "content": {PRETTY}, "\u0063ontent": {PRETTY}},
    {"type": "tool_result", "tool_use_id": "c", "content": {BASH}}
  ]}
]}"#;
    let (input, expected) = render(template);
    let mut p = proxy(true).await;
    let seen = relay(&mut p, messages(), input).await;
    assert_eq!(String::from_utf8(seen.body.to_vec()).unwrap(), expected);
}

#[tokio::test]
async fn body_over_the_inspection_cap_is_forwarded_intact() {
    let (input, _) = render(TURN_1);
    let pad = "x".repeat(33 * 1024 * 1024);
    let input = input.replacen("\"model\"", &format!("\"pad\": \"{pad}\", \"model\""), 1);
    let mut p = proxy(true).await;
    let seen = relay(&mut p, messages(), input.clone()).await;
    assert!(seen.body == input.as_bytes(), "over-cap body changed");
}

#[tokio::test]
async fn requests_with_body_integrity_headers_pass_through() {
    let (input, _) = render(TURN_1);
    let mut p = proxy(true).await;
    for name in [
        "content-digest",
        "digest",
        "content-md5",
        "x-amz-content-sha256",
        "repr-digest",
    ] {
        let req = messages().header(name, "x");
        assert_eq!(
            relay(&mut p, req, input.clone()).await.body,
            input,
            "{name}"
        );
    }
}

#[tokio::test]
async fn tool_result_content_that_is_not_text_passes_through() {
    let input = r#"{"messages": [
  {"role": "assistant", "content": [
    {"type": "tool_use", "id": "a", "name": "Bash", "input": {}},
    {"type": "tool_use", "id": "b", "name": "Bash", "input": {}}
  ]},
  {"role": "user", "content": [
    {"type": "tool_result", "tool_use_id": "a", "content": { "x" : [ 1 ] }},
    {"type": "tool_result", "tool_use_id": "b", "content": 42}
  ]}
]}"#;
    let mut p = proxy(true).await;
    assert_eq!(relay(&mut p, messages(), input).await.body, input);
}

#[tokio::test]
async fn chunked_request_is_rewritten_with_content_length() {
    let (input, expected) = render(TURN_1);
    let (p, mut seen) = proxy(true).await;
    let stream = TcpStream::connect(p).await.unwrap();
    let (mut tx, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(conn);
    let (mut sender, body) = Channel::<Bytes, Infallible>::new(2);
    let (head, tail) = input.split_at(input.len() / 2);
    let (head, tail) = (Bytes::from(head.to_owned()), Bytes::from(tail.to_owned()));
    tokio::spawn(async move {
        sender.send_data(head).await.unwrap();
        sender.send_data(tail).await.unwrap();
    });
    let resp = tx
        .send_request(messages().body(body).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let up = seen.recv().await.unwrap();
    assert_eq!(String::from_utf8(up.body.to_vec()).unwrap(), expected);
    let length = expected.len().to_string();
    assert_eq!(
        up.headers
            .get("content-length")
            .map(|v| v.to_str().unwrap()),
        Some(length.as_str())
    );
    assert!(up.headers.get("transfer-encoding").is_none());
}

#[tokio::test]
async fn request_with_trailers_is_not_rewritten() {
    let (input, _) = render(TURN_1);
    let (p, mut seen) = proxy(true).await;
    let stream = TcpStream::connect(p).await.unwrap();
    let (mut tx, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(conn);
    let (mut sender, body) = Channel::<Bytes, Infallible>::new(2);
    let data = Bytes::from(input.clone());
    tokio::spawn(async move {
        sender.send_data(data).await.unwrap();
        let mut trailers = HeaderMap::new();
        trailers.insert("content-digest", "sha-256=:x:".parse().unwrap());
        sender.send_trailers(trailers).await.unwrap();
    });
    let req = messages().header("trailer", "content-digest");
    let resp = tx.send_request(req.body(body).unwrap()).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(seen.recv().await.unwrap().body, input);
}

#[tokio::test]
async fn busy_executor_forwards_the_original_body() {
    let (input, expected) = render(TURN_1);
    let exec = BoundedExecutor::new(1, Duration::from_secs(60)).unwrap();
    let mut p = proxy_with(true, exec.clone()).await;
    // Clones share one permit pool, so this job holds the proxy's only permit.
    let (release, wait) = std::sync::mpsc::channel::<()>();
    let holder = {
        let exec = exec.clone();
        std::thread::spawn(move || exec.run(move || wait.recv()))
    };
    while exec.in_flight() == 0 {
        std::thread::yield_now();
    }
    assert_eq!(relay(&mut p, messages(), input.clone()).await.body, input);
    drop(release);
    assert!(holder.join().unwrap().is_ok());
    assert_eq!(relay(&mut p, messages(), input).await.body, expected);
}
