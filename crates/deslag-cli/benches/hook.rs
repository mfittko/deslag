//! Whole-process `deslag hook claude` latency (PLAN §13.1, §13.2 warm hook case).
//! Includes process spawn, stdin/stdout pipes and exit. No thresholds.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use deslag_proxy::session::{Owner, write_owner};

const WARMUP: usize = 10;
const SAMPLES: usize = 200;

/// A Bash PostToolUse event whose stdout is about 64 KiB of pretty JSON.
fn event() -> Vec<u8> {
    let mut stdout = String::from("[\n");
    let mut i = 0;
    while stdout.len() < 64 * 1024 {
        stdout.push_str(&format!(
            "  {{\n    \"id\": {i},\n    \"name\": \"user-{i}\",\n    \"active\": true\n  }},\n"
        ));
        i += 1;
    }
    stdout.push_str("  null\n]\n");
    let event = serde_json::json!({
        "session_id": "00000000-0000-4000-8000-000000000001",
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": "cat records.json"},
        "tool_response": {
            "stdout": stdout,
            "stderr": "",
            "interrupted": false,
            "isImage": false,
            "noOutputExpected": false,
        },
        "tool_use_id": "toolu_bench",
    });
    serde_json::to_vec(&event).unwrap()
}

fn invoke(dir: &Path, args: &[&str], input: &[u8]) -> Duration {
    let start = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_deslag"))
        .env("DESLAG_STATE_DIR", dir)
        .args(args)
        // Isolate from the host Claude settings: the conflict scan reads them.
        .env("CLAUDE_CONFIG_DIR", dir)
        .env("CLAUDE_PROJECT_DIR", dir)
        .env("HOME", dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    let elapsed = start.elapsed();
    assert!(output.status.success());
    // Only --minify-json may emit a replacement, and it must for this event.
    assert_eq!(output.stdout.is_empty(), args.len() == 2);
    elapsed
}

fn percentile(sorted: &[Duration], q: f64) -> Duration {
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

fn main() {
    let input = event();
    // The hook owns compression, so --minify-json replaces the result.
    let dir = std::env::temp_dir().join(format!("deslag-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    write_owner(&dir, Owner::Hook).unwrap();
    println!(
        "### deslag hook claude, whole process ({} byte event, concurrency 1, warmup {WARMUP}, samples {SAMPLES})\n",
        input.len()
    );
    println!("| mode | p50 | p95 | p99 |");
    println!("| --- | --- | --- | --- |");
    for (mode, args) in [
        ("no-op (no flag)", &["hook", "claude"][..]),
        ("--minify-json", &["hook", "claude", "--minify-json"]),
    ] {
        for _ in 0..WARMUP {
            invoke(&dir, args, &input);
        }
        let mut times: Vec<Duration> = (0..SAMPLES).map(|_| invoke(&dir, args, &input)).collect();
        times.sort();
        println!(
            "| {mode} | {:?} | {:?} | {:?} |",
            percentile(&times, 0.50),
            percentile(&times, 0.95),
            percentile(&times, 0.99),
        );
    }
    println!();
    let _ = std::fs::remove_dir_all(&dir);
}
