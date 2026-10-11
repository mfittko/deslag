//! Request paths must stay inside the configured upstream base path.

use std::{convert::Infallible, net::SocketAddr, time::Duration};

use bytes::Bytes;
use deslag_proxy::{Upstream, serve};
use http_body_util::Full;
use hyper::{Request, Response, body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    time::timeout,
};

const WAIT: Duration = Duration::from_secs(5);
const REJECTED: &[u8] = b"HTTP/1.1 400 Bad Request\r\n";
const FIXED_BODY: &str = r#"{"type":"error","error":{"type":"invalid_request_error","message":"unsupported request target"}}"#;

/// Mock upstream that records each request URI and answers 200.
async fn mock() -> (SocketAddr, mpsc::UnboundedReceiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let tx = tx.clone();
            let svc = service_fn(move |req: Request<Incoming>| {
                let _ = tx.send(req.uri().to_string());
                async { Ok::<_, Infallible>(Response::new(Full::new(Bytes::from("ok")))) }
            });
            tokio::spawn(http1::Builder::new().serve_connection(TokioIo::new(stream), svc));
        }
    });
    (addr, rx)
}

async fn proxy_to(upstream: SocketAddr) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let upstream = Upstream::parse(&format!("http://{upstream}/base")).unwrap();
    tokio::spawn(serve(listener, upstream));
    addr
}

/// Sends `target` verbatim in the request line so no client normalizes it.
async fn raw_get(proxy: SocketAddr, target: &str) -> Vec<u8> {
    let mut stream = TcpStream::connect(proxy).await.unwrap();
    let req = format!("GET {target} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).await.unwrap();
    let mut resp = Vec::new();
    timeout(WAIT, stream.read_to_end(&mut resp))
        .await
        .unwrap()
        .unwrap();
    resp
}

#[tokio::test]
async fn dot_dot_paths_are_rejected_without_reaching_upstream() {
    let (upstream, mut seen) = mock().await;
    let proxy = proxy_to(upstream).await;

    for target in [
        "/../admin",
        "/v1/../../admin",
        "/v1/%2e%2e/admin",
        "/v1/%2E%2e/admin",
        "/v1/.%2e/admin",
        "/v1/..%2fadmin",
        "/v1/..%5cadmin",
        "/v1/..;/admin",
        "/v1/..\\admin",
        "http://x/../admin",
        "/..",
    ] {
        let resp = raw_get(proxy, target).await;
        assert!(resp.starts_with(REJECTED), "{target} not rejected");
        let body = String::from_utf8(resp).unwrap();
        let body = body.split_once("\r\n\r\n").unwrap().1;
        assert_eq!(body, FIXED_BODY, "{target} body");
    }

    // Nothing reached the upstream: the next forwarded request is the first one seen.
    assert!(seen.try_recv().is_err());
    let resp = raw_get(proxy, "/v1/models").await;
    assert!(resp.starts_with(b"HTTP/1.1 200 OK\r\n"));
    let first = timeout(WAIT, seen.recv()).await.unwrap().unwrap();
    assert_eq!(first, "/base/v1/models");
}

#[tokio::test]
async fn normal_paths_forward_unchanged() {
    let (upstream, mut seen) = mock().await;
    let proxy = proxy_to(upstream).await;

    for target in [
        "/v1/messages?beta=true",
        "/v1/foo.bar",
        "/v1/..foo",
        "/v1/a..b",
        "/v1/./messages",
        "/v1/%2e/x",
        "/v1/messages?next=../x",
    ] {
        let resp = raw_get(proxy, target).await;
        assert!(
            resp.starts_with(b"HTTP/1.1 200 OK\r\n"),
            "{target} not forwarded"
        );
        let got = timeout(WAIT, seen.recv()).await.unwrap().unwrap();
        assert_eq!(got, format!("/base{target}"));
    }
}
