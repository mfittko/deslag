use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use deslag_core::{EstimateCounter, FIRST_SLICE_JSON_MINIFY_CONTEXT, compress};
use deslag_telemetry::{Adapter, MAX_MESSAGE_BYTES, ToolCategory, compress_reported, encode, send};
use serde_json::Value;

/// A fresh short directory under /tmp, removed on drop: macOS caps socket
/// paths at 104 bytes.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_dir(name: &str) -> TempDir {
    let dir = PathBuf::from(format!("/tmp/dsl-core-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(dir)
}

const PRETTY: &str = "{\n  \"name\": \"secret-payload\",\n  \"tags\": [1, 2, 3]\n}\n";

fn recv(sock: &UnixDatagram) -> Value {
    let mut buf = [0; 1024];
    sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let n = sock.recv(&mut buf).unwrap();
    serde_json::from_slice(&buf[..n]).unwrap()
}

#[test]
fn encoded_report_is_small_and_carries_labels_only() {
    let mut ctx = FIRST_SLICE_JSON_MINIFY_CONTEXT;
    ctx.token_counter = Some(&EstimateCounter);
    let report = compress(PRETTY, &ctx).report;
    let message = encode(&report, Adapter::Hook, ToolCategory::Bash, 3);
    assert!(message.len() <= MAX_MESSAGE_BYTES, "{message}");
    assert!(!message.contains("secret-payload"));
    assert!(!message.contains("Bash"));
    let v: Value = serde_json::from_str(&message).unwrap();
    assert_eq!(v["adapter"], "hook");
    assert_eq!(v["tool"], "bash");
    assert_eq!(v["transform"], "json-minify");
    assert_eq!(v["outcome"], "changed");
    assert_eq!(v["reason"], Value::Null);
    assert_eq!(v["original_bytes"], PRETTY.len());
    assert_eq!(v["dropped"], 3);
    assert_eq!(v["tokens"]["method"], "estimate");
    assert_eq!(v["tokens"]["tokenizer"], EstimateCounter::TOKENIZER);
    assert!(v["tokens"]["candidate"].is_u64());

    let plain = compress("x", &FIRST_SLICE_JSON_MINIFY_CONTEXT).report;
    let v: Value =
        serde_json::from_str(&encode(&plain, Adapter::Proxy, ToolCategory::Other, 0)).unwrap();
    assert_eq!(v["tokens"], Value::Null);
    assert_eq!(v["candidate_bytes"], Value::Null);
    assert_eq!(v["reason"], "unsupported_format");
}

#[test]
fn tool_names_map_to_bounded_categories() {
    assert_eq!(ToolCategory::from_tool_name("Bash"), ToolCategory::Bash);
    assert_eq!(ToolCategory::from_tool_name("mcp__x__y"), ToolCategory::Mcp);
    assert_eq!(ToolCategory::from_tool_name("Read"), ToolCategory::Other);
    assert_eq!(ToolCategory::from_tool_name("bash"), ToolCategory::Other);
}

#[test]
fn send_fails_closed_on_oversize_and_missing_socket() {
    let dir = temp_dir("send");
    let path = dir.0.join("s.sock");
    assert!(!send(&path, b"{}"));
    let receiver = UnixDatagram::bind(&path).unwrap();
    assert!(!send(&path, &[b' '; MAX_MESSAGE_BYTES + 1]));
    assert!(send(&path, &[b' '; MAX_MESSAGE_BYTES]));
    let mut buf = [0; MAX_MESSAGE_BYTES + 1];
    assert_eq!(receiver.recv(&mut buf).unwrap(), MAX_MESSAGE_BYTES);
    drop(receiver);
    // The stale socket file remains; nobody listens on it.
    assert!(path.exists());
    assert!(!send(&path, b"{}"));
}

#[test]
fn send_never_blocks_on_a_full_buffer() {
    let dir = temp_dir("full");
    let path = dir.0.join("s.sock");
    let _receiver = UnixDatagram::bind(&path).unwrap();
    let start = Instant::now();
    let refused = (0..100_000).any(|_| !send(&path, &[b'x'; MAX_MESSAGE_BYTES]));
    assert!(refused, "the receive buffer never filled");
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn reported_compression_matches_plain_and_carries_drops() {
    let dir = temp_dir("carry");
    let path = dir.0.join("s.sock");
    let ctx = FIRST_SLICE_JSON_MINIFY_CONTEXT;
    let plain = compress(PRETTY, &ctx).output.into_owned();
    let run = |socket| {
        compress_reported(PRETTY, &ctx, socket, Adapter::Proxy, ToolCategory::Bash)
            .output
            .into_owned()
    };
    assert_eq!(run(None), plain);

    drop(UnixDatagram::bind(&path).unwrap());
    assert_eq!(run(Some(&path)), plain);
    assert_eq!(run(Some(&path)), plain);

    std::fs::remove_file(&path).unwrap();
    let receiver = UnixDatagram::bind(&path).unwrap();
    assert_eq!(run(Some(&path)), plain);
    assert_eq!(recv(&receiver)["dropped"], 2);
    assert_eq!(run(Some(&path)), plain);
    let v = recv(&receiver);
    assert_eq!(v["dropped"], 0);
    assert_eq!(v["adapter"], "proxy");
    assert!(v["tokens"]["original"].is_u64());
}
