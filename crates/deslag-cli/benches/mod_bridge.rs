//! Spike benchmark (docs/spikes/0086-claude-mod-first.md): whole-process
//! `deslag hook claude --minify-json` against `deslag codec --protocol 1` on
//! the same pretty-JSON payloads. Includes spawn, pipes and exit; excludes the
//! client's own `$.process.run` and hook dispatch. No thresholds.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::json;

/// Pretty JSON records of about `bytes` bytes.
fn payload(bytes: usize) -> String {
    let mut text = String::from("[\n");
    let mut i = 0;
    while text.len() < bytes {
        text.push_str(&format!(
            "  {{\n    \"id\": {i},\n    \"name\": \"user-{i}\",\n    \"active\": true\n  }},\n"
        ));
        i += 1;
    }
    text.push_str("  null\n]\n");
    text
}

fn hook_input(text: &str) -> Vec<u8> {
    let event = json!({
        "session_id": "00000000-0000-4000-8000-000000000001",
        "hook_event_name": "PostToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": "cat records.json"},
        "tool_response": {"stdout": text, "stderr": "", "interrupted": false,
                          "isImage": false, "noOutputExpected": false},
        "tool_use_id": "toolu_bench",
    });
    serde_json::to_vec(&event).unwrap()
}

fn codec_input(text: &str, mode: &str) -> Vec<u8> {
    let request = json!({"schema_version": 1, "policy_version": "bench", "mode": mode,
                         "tool_category": "bash", "items": [{"slot": 0, "text": text}]});
    serde_json::to_vec(&request).unwrap()
}

/// One whole-process call: elapsed time, exit success, stdout bytes.
fn invoke(dir: &Path, args: &[&str], input: &[u8]) -> (Duration, bool, usize) {
    let start = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_deslag"))
        .args(args)
        .env("DESLAG_STATE_DIR", dir)
        .env("CLAUDE_CONFIG_DIR", dir)
        .env("CLAUDE_PROJECT_DIR", dir)
        .env("HOME", dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // A writer thread: the child may stop reading early (oversize input).
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let writer = std::thread::spawn(move || drop(stdin.write_all(&input)));
    let output = child.wait_with_output().unwrap();
    let elapsed = start.elapsed();
    writer.join().unwrap();
    (elapsed, output.status.success(), output.stdout.len())
}

fn percentile(sorted: &[Duration], q: f64) -> Duration {
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

fn main() {
    let dir = std::env::temp_dir().join(format!("deslag-mod-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    println!("### hook vs codec bridge, whole process, concurrency 1\n");
    println!("| payload | path | owner | samples | p50 | p95 | p99 | stdout bytes | exit ok |");
    println!("| --- | --- | --- | --- | --- | --- | --- | --- | --- |");
    // ponytail: sample counts shrink with size to keep the run short.
    for (label, bytes, samples) in [
        ("2 KiB", 2 * 1024, 200),
        ("64 KiB", 64 * 1024, 200),
        ("1 MiB", 1024 * 1024, 50),
        ("17 MiB (above the 16 MiB cap)", 17 * 1024 * 1024, 5),
    ] {
        let text = payload(bytes);
        let cases: [(&str, &str, &[&str], Vec<u8>); 3] = [
            (
                "hook --minify-json",
                "hook",
                &["hook", "claude", "--minify-json"],
                hook_input(&text),
            ),
            (
                "codec safe",
                "mod",
                &["codec", "--protocol", "1"],
                codec_input(&text, "safe"),
            ),
            (
                "codec observe",
                "mod",
                &["codec", "--protocol", "1"],
                codec_input(&text, "observe"),
            ),
        ];
        for (path, owner, args, input) in cases {
            std::fs::write(dir.join("owner"), owner).unwrap();
            for _ in 0..samples.min(10) {
                invoke(&dir, args, &input);
            }
            let runs: Vec<_> = (0..samples).map(|_| invoke(&dir, args, &input)).collect();
            let mut times: Vec<Duration> = runs.iter().map(|r| r.0).collect();
            times.sort();
            let (_, ok, out) = runs[0];
            println!(
                "| {label} | {path} | {owner} | {samples} | {:?} | {:?} | {:?} | {out} | {ok} |",
                percentile(&times, 0.50),
                percentile(&times, 0.95),
                percentile(&times, 0.99),
            );
        }
    }
    println!();
    let _ = std::fs::remove_dir_all(&dir);
}
