//! Connection and timeout limits, exercised with short injected values.

use std::{convert::Infallible, net::SocketAddr, time::Duration};

use bytes::Bytes;
use deslag_proxy::{Limits, Upstream, serve_with};
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
const SHORT: Duration = Duration::from_millis(100);
const REQUEST: &[u8] = b"GET /v1/models HTTP/1.1\r\nhost: proxy\r\n\r\n";

/// Mock upstream answering `ok`; sends one message whenever a connection closes.
async fn mock() -> (SocketAddr, mpsc::UnboundedReceiver<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let svc = service_fn(|_: Request<Incoming>| async {
                Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"ok"))))
            });
            let tx = tx.clone();
            tokio::spawn(async move {
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), svc)
                    .await;
                let _ = tx.send(());
            });
        }
    });
    (addr, rx)
}

async fn proxy(upstream: &str, limits: Limits) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let upstream = Upstream::parse(upstream).unwrap();
    tokio::spawn(serve_with(listener, upstream, limits));
    addr
}

/// Sends one request on `stream` and reads until the response head is complete.
async fn exchange(stream: &mut TcpStream) -> String {
    stream.write_all(REQUEST).await.unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut chunk).await.unwrap();
        assert!(n > 0, "connection closed before a response");
        buf.extend_from_slice(&chunk[..n]);
    }
    String::from_utf8(buf).unwrap()
}

#[tokio::test]
async fn slow_header_client_is_disconnected() {
    let (up, _) = mock().await;
    let limits = Limits {
        header_read_timeout: SHORT,
        ..Limits::default()
    };
    let addr = proxy(&format!("http://{up}"), limits).await;
    let mut client = TcpStream::connect(addr).await.unwrap();
    let start = std::time::Instant::now();
    client
        .write_all(b"POST /v1/messages HTTP/1.1\r\n")
        .await
        .unwrap();
    let mut rest = Vec::new();
    // EOF or reset both mean the proxy dropped the connection.
    let _ = timeout(WAIT, client.read_to_end(&mut rest))
        .await
        .expect("proxy kept a slow-header connection open");
    assert!(start.elapsed() >= SHORT, "closed before the header timeout");
}

#[tokio::test]
async fn connections_above_cap_wait_for_a_free_slot() {
    let (up, _) = mock().await;
    let limits = Limits {
        max_connections: 1,
        ..Limits::default()
    };
    let addr = proxy(&format!("http://{up}"), limits).await;
    let mut first = TcpStream::connect(addr).await.unwrap();
    assert!(exchange(&mut first).await.starts_with("HTTP/1.1 200"));

    let mut second = TcpStream::connect(addr).await.unwrap();
    let pending = tokio::spawn(async move { exchange(&mut second).await });
    tokio::time::sleep(SHORT).await;
    assert!(!pending.is_finished(), "second connection served above cap");

    drop(first);
    let head = timeout(WAIT, pending).await.unwrap().unwrap();
    assert!(head.starts_with("HTTP/1.1 200"));
}

#[tokio::test]
async fn unreachable_upstream_fails_within_connect_timeout() {
    let limits = Limits {
        connect_timeout: SHORT,
        ..Limits::default()
    };
    // TEST-NET-1 is never routed. The timeout is exercised only where the SYN is
    // blackholed; a host that fails fast with ENETUNREACH passes trivially.
    let addr = proxy("https://192.0.2.1", limits).await;
    let mut client = TcpStream::connect(addr).await.unwrap();
    let head = timeout(WAIT, exchange(&mut client))
        .await
        .expect("connect did not time out");
    assert!(head.starts_with("HTTP/1.1 502"), "{head}");
}

#[tokio::test]
async fn idle_upstream_connection_is_closed() {
    let (up, mut closed) = mock().await;
    let limits = Limits {
        pool_idle_timeout: SHORT,
        ..Limits::default()
    };
    let addr = proxy(&format!("http://{up}"), limits).await;
    let mut client = TcpStream::connect(addr).await.unwrap();
    assert!(exchange(&mut client).await.starts_with("HTTP/1.1 200"));
    // The client connection stays open, so only the pool can close the upstream leg.
    timeout(WAIT, closed.recv())
        .await
        .expect("idle upstream connection not closed")
        .unwrap();
    drop(client);
}
