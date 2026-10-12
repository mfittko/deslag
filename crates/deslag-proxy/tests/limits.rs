//! Connection and timeout limits, exercised with short injected values.

use std::{convert::Infallible, net::SocketAddr, time::Duration};

use bytes::Bytes;
use deslag_proxy::{
    Limits, Upstream, available_inspect_permits, serve_with,
    session::{Mode, Store},
};
use http_body_util::Full;
use hyper::{Request, Response, body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpSocket, TcpStream},
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

/// Sends `REQUEST` on `stream` and returns the response head.
async fn exchange(stream: &mut TcpStream) -> String {
    send(stream, REQUEST).await
}

/// Sends `request` on `stream` and reads until the response head is complete.
async fn send(stream: &mut TcpStream, request: &[u8]) -> String {
    stream.write_all(request).await.unwrap();
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
    let start = std::time::Instant::now();
    let mut client = TcpStream::connect(addr).await.unwrap();
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

/// Upstream whose accept queue is full, so the kernel drops further SYNs and a
/// connect stalls until the caller gives up. The returned guards keep it full.
async fn stalled_upstream() -> (SocketAddr, Vec<TcpStream>, TcpListener) {
    let socket = TcpSocket::new_v4().unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let listener = socket.listen(1).unwrap();
    let addr = listener.local_addr().unwrap();
    let mut held = Vec::new();
    // Two stalls in a row, so one slow connect on a loaded host is not mistaken for a full queue.
    let mut stalls = 0;
    for _ in 0..64 {
        match timeout(SHORT, TcpStream::connect(addr)).await {
            Ok(stream) => {
                held.push(stream.unwrap());
                stalls = 0;
            }
            Err(_) if stalls == 1 => return (addr, held, listener),
            Err(_) => stalls += 1,
        }
    }
    panic!("listen backlog never filled; cannot stall a connect");
}

#[tokio::test]
async fn stalled_upstream_connect_fails_at_connect_timeout() {
    let (up, _held, _listener) = stalled_upstream().await;
    let connect_timeout = SHORT * 5;
    let limits = Limits {
        connect_timeout,
        ..Limits::default()
    };
    let addr = proxy(&format!("http://{up}"), limits).await;
    let start = std::time::Instant::now();
    let mut client = TcpStream::connect(addr).await.unwrap();
    let pending = tokio::spawn(async move { exchange(&mut client).await });
    tokio::time::sleep(SHORT).await;
    assert!(
        !pending.is_finished(),
        "upstream connect failed before the deadline"
    );
    let head = timeout(WAIT, pending)
        .await
        .expect("connect did not time out")
        .unwrap();
    assert!(head.starts_with("HTTP/1.1 502"), "{head}");
    assert!(
        start.elapsed() >= connect_timeout,
        "502 before the connect timeout"
    );
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

#[tokio::test]
async fn stalled_minify_uploads_release_permits_at_body_read_timeout() {
    let (up, _) = mock().await;
    // ponytail: waits on the hidden permit count instead of a fixed sleep. The
    // healthy request can only finish after a stalled permit times out.
    let body_read_timeout = Duration::from_secs(1);
    let limits = Limits {
        body_read_timeout,
        ..Limits::default()
    };
    let upstream = Upstream::parse(&format!("http://{up}"))
        .unwrap()
        .with_json_minification(Store {
            dir: std::env::temp_dir().join(format!("deslag-limits-{}", std::process::id())),
            mode: Mode::Safe,
        });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(serve_with(listener, upstream, limits));

    // Taken before any permit is granted, so no stalled permit can time out
    // before start + body_read_timeout.
    let start = std::time::Instant::now();
    // Four uploads take every inspection permit, then stall mid-body.
    let stalled: Vec<_> = (0..4)
        .map(|_| {
            tokio::spawn(async move {
                let mut client = TcpStream::connect(addr).await.unwrap();
                send(
                    &mut client,
                    b"POST /v1/messages HTTP/1.1\r\nhost: proxy\r\nx-claude-code-session-id: s\r\ncontent-length: 1000\r\n\r\n{\"messages\"",
                )
                .await
            })
        })
        .collect();
    // Waits until the stalled uploads hold every permit.
    timeout(WAIT, async {
        while available_inspect_permits() > 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("stalled uploads never took every permit");

    // A request without a session id needs no permit, so it streams through
    // while every permit is held.
    let mut unscoped = TcpStream::connect(addr).await.unwrap();
    let head = send(
        &mut unscoped,
        b"POST /v1/messages HTTP/1.1\r\nhost: proxy\r\ncontent-length: 2\r\n\r\n{}",
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(
        start.elapsed() < body_read_timeout,
        "unscoped request queued for a permit"
    );

    let mut healthy = TcpStream::connect(addr).await.unwrap();
    let head = timeout(
        WAIT,
        send(
            &mut healthy,
            b"POST /v1/messages HTTP/1.1\r\nhost: proxy\r\nx-claude-code-session-id: s\r\ncontent-length: 2\r\n\r\n{}",
        ),
    )
    .await
    .expect("healthy request still blocked after the body-read timeout");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(
        start.elapsed() >= body_read_timeout,
        "healthy request did not queue behind the stalled uploads"
    );
    for task in stalled {
        let head = timeout(WAIT, task).await.unwrap().unwrap();
        assert!(head.starts_with("HTTP/1.1 408"), "{head}");
    }
}
