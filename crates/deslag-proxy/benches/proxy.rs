//! Proxy request latency against a local mock upstream (PLAN §13.1, §13.2 proxy case).
//! Compares direct-to-mock, transparent proxy and proxy with --minify-json on the
//! same ~256 KiB Messages request with ~64 KiB of eligible Bash JSON. No thresholds.

use std::{
    convert::Infallible,
    net::SocketAddr,
    time::{Duration, Instant},
};

use bytes::Bytes;
use deslag_proxy::{
    Upstream, serve,
    session::{Mode, Owner, Store, write_owner},
};
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, TcpStream};

const WARMUP: usize = 20;
const SAMPLES: usize = 500;

fn request_body() -> Bytes {
    let mut result = String::from("[\n");
    let mut i = 0;
    while result.len() < 64 * 1024 {
        result.push_str(&format!(
            "  {{\n    \"id\": {i},\n    \"name\": \"user-{i}\",\n    \"active\": true\n  }},\n"
        ));
        i += 1;
    }
    result.push_str("  null\n]\n");
    let history = "Earlier conversation text. ".repeat(190 * 1024 / 27);
    let body = serde_json::json!({
        "model": "claude-bench",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": history},
            {"role": "assistant", "content": [
                {"type": "tool_use", "id": "toolu_bench", "name": "Bash", "input": {"command": "cat records.json"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_bench", "content": result}
            ]}
        ]
    });
    serde_json::to_vec(&body).unwrap().into()
}

/// Mock upstream that answers immediately with the received body length.
async fn mock() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let _ = stream.set_nodelay(true);
            let svc = service_fn(|req: Request<Incoming>| async move {
                let len = req.into_body().collect().await.unwrap().to_bytes().len();
                Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(len.to_string()))))
            });
            tokio::spawn(http1::Builder::new().serve_connection(TokioIo::new(stream), svc));
        }
    });
    addr
}

async fn proxy(upstream: SocketAddr, minify: bool) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut upstream = Upstream::parse(&format!("http://{upstream}")).unwrap();
    if minify {
        let dir = std::env::temp_dir().join(format!("deslag-bench-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_owner(&dir, Owner::Proxy).unwrap();
        let store = Store {
            dir,
            mode: Mode::Safe,
        };
        upstream = upstream.with_json_minification(store);
    }
    tokio::spawn(serve(listener, upstream));
    addr
}

type Sender = hyper::client::conn::http1::SendRequest<Full<Bytes>>;

/// Opens one kept-alive connection to `target`.
async fn connect(target: SocketAddr) -> Sender {
    let stream = TcpStream::connect(target).await.unwrap();
    stream.set_nodelay(true).unwrap();
    let (tx, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(conn);
    tx
}

/// Sends `n` sequential requests on one connection.
/// Returns sorted latencies and the body length the upstream last reported.
async fn measure(tx: &mut Sender, body: &Bytes, n: usize) -> (Vec<Duration>, usize) {
    let mut times = Vec::with_capacity(n);
    let mut seen = 0;
    for _ in 0..n {
        tx.ready().await.unwrap();
        let req = Request::post("/v1/messages")
            .header("content-type", "application/json")
            .header("x-claude-code-session-id", "bench")
            .body(Full::new(body.clone()))
            .unwrap();
        let start = Instant::now();
        let resp = tx.send_request(req).await.unwrap();
        let reply = resp.into_body().collect().await.unwrap().to_bytes();
        times.push(start.elapsed());
        seen = std::str::from_utf8(&reply).unwrap().parse().unwrap();
    }
    times.sort();
    (times, seen)
}

fn percentile(sorted: &[Duration], q: f64) -> Duration {
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

#[tokio::main]
async fn main() {
    let body = request_body();
    let upstream = mock().await;
    let transparent = proxy(upstream, false).await;
    let minifying = proxy(upstream, true).await;
    let mut targets = Vec::new();
    for (label, target) in [
        ("direct to mock", upstream),
        ("proxy, transparent", transparent),
        ("proxy, --minify-json", minifying),
    ] {
        targets.push((label, connect(target).await));
    }
    // Start the session with a request without tool results, so the first
    // measured request records its decision and later ones replay it.
    let first = Bytes::from_static(
        br#"{"model":"claude-bench","messages":[{"role":"user","content":"hi"}]}"#,
    );
    for (_, tx) in &mut targets {
        measure(tx, &first, 1).await;
    }
    // Warm up every target before measuring any of them.
    for (_, tx) in &mut targets {
        measure(tx, &body, WARMUP).await;
    }
    println!(
        "### deslag-proxy request latency ({} byte request, concurrency 1, warmup {WARMUP}, samples {SAMPLES})\n",
        body.len()
    );
    println!("| path | upstream bytes | p50 | p95 | p99 |");
    println!("| --- | --- | --- | --- | --- |");
    for (label, tx) in &mut targets {
        let (times, seen) = measure(tx, &body, SAMPLES).await;
        println!(
            "| {label} | {seen} | {:?} | {:?} | {:?} |",
            percentile(&times, 0.50),
            percentile(&times, 0.95),
            percentile(&times, 0.99),
        );
    }
    println!();
}
