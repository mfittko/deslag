use std::io::Write;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Returns a fresh directory unique to this call, so parallel test threads never
/// remove a directory another thread is using. It lives under /tmp because macOS
/// caps socket paths at 104 bytes.
fn temp_dir(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = PathBuf::from(format!("/tmp/deslag-it-{}-{n}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Spawns the hook with config, project, home and state dirs at `dir`, so the
/// developer's real Claude Code settings and local service never apply.
fn spawn_in(dir: &Path, args: &[&str]) -> Child {
    Command::new(env!("CARGO_BIN_EXE_deslag"))
        .args(args)
        .env("CLAUDE_CONFIG_DIR", dir)
        .env("CLAUDE_PROJECT_DIR", dir)
        .env("HOME", dir)
        .env("DESLAG_STATE_DIR", dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn deslag")
}

fn run_in(dir: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut child = spawn_in(dir, args);
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

fn run(args: &[&str], stdin: &[u8]) -> Output {
    run_in(&temp_dir("empty"), args, stdin)
}

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../tests/fixtures/claude/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).unwrap()
}

#[test]
fn noop_on_captured_payloads() {
    for name in ["posttooluse-bash.json", "posttooluse-read.json"] {
        let output = run(&["hook", "claude"], &fixture(name));
        assert_eq!(output.status.code(), Some(0), "{name}");
        assert!(output.stdout.is_empty(), "{name}");
        assert!(output.stderr.is_empty(), "{name}");
    }
}

#[test]
fn malformed_input_fails_open_with_diagnostic_on_stderr() {
    for input in [
        &b"{not json"[..],
        b"",
        b"[]",
        b"{\"hook_event_name\":\"Post\\nX\"}",
    ] {
        for args in [
            &["hook", "claude"][..],
            &["hook", "claude", "--minify-json"],
        ] {
            let output = run(args, input);
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stdout.is_empty());
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(stderr.lines().count(), 1, "{stderr}");
        }
    }
}

#[test]
fn unknown_arguments_print_usage_and_exit_64() {
    for args in [
        &[][..],
        &["hook"],
        &["hook", "codex"],
        &["hook", "claude", "x"],
        &["hook", "claude", "--minify-json", "x"],
        &["hook", "claude", "--minify-json", "--minify-json"],
    ] {
        let output = run(args, b"");
        assert_eq!(output.status.code(), Some(64), "{args:?}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage"));
    }
}

#[test]
fn oversize_input_is_drained_and_fails_open() {
    // Well past the 16 MiB cap and the pipe buffer: without the drain, write_all in run() hits EPIPE.
    let input = vec![b' '; 17 * 1024 * 1024];
    let output = run(&["hook", "claude"], &input);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
}

/// The spawned hook runs every job under the 10 ms default budget. Parallel tests
/// contend for the CPU, so a job can miss it. That case fails open: no stdout and
/// one budget diagnostic. Returns true for that case. Otherwise asserts empty
/// stderr. Exact replacements are asserted in-process in `main.rs` under a
/// generous budget, so these tests never depend on wall-clock time.
fn budget_missed(output: &Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.stdout.is_empty() && !stderr.is_empty() {
        assert_eq!(
            stderr,
            "deslag: compression budget_exceeded; tool output unchanged\n"
        );
        return true;
    }
    assert!(stderr.is_empty(), "{stderr}");
    false
}

fn json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn minify_flag_replaces_pretty_json_bash_stdout() {
    let input = fixture("posttooluse-bash-json.json");
    let output = run(&["hook", "claude", "--minify-json"], &input);
    assert_eq!(output.status.code(), Some(0));
    if budget_missed(&output) {
        return;
    }
    assert_eq!(output.stdout.iter().filter(|&&b| b == b'\n').count(), 1);

    let mut expected = json(&input)["tool_response"].clone();
    expected["stdout"] = r#"{"name":"deslag","tags":["a b","c"],"size":1.50}"#.into();
    assert_eq!(
        json(&output.stdout),
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PostToolUse",
                "updatedToolOutput": expected,
            }
        })
    );

    let again = run(&["hook", "claude", "--minify-json"], &input);
    if !budget_missed(&again) {
        assert_eq!(again.stdout, output.stdout);
    }
}

#[test]
fn minify_flag_leaves_ineligible_results_alone() {
    let mut read_json = json(&fixture("posttooluse-read.json"));
    read_json["tool_response"]["file"]["content"] = "{\n  \"a\": 1\n}\n".into();
    for input in [
        fixture("posttooluse-bash.json"),
        serde_json::to_vec(&read_json).unwrap(),
    ] {
        let output = run(&["hook", "claude", "--minify-json"], &input);
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stdout.is_empty());
        // The Bash stdout still runs one job, which can miss the budget.
        budget_missed(&output);
    }
}

#[test]
fn without_flag_pretty_json_is_untouched() {
    let output = run(&["hook", "claude"], &fixture("posttooluse-bash-json.json"));
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn live_service_gets_a_hook_report_and_output_is_unchanged() {
    let args = ["hook", "claude", "--minify-json"];
    let dir = temp_dir("live");
    let service = UnixDatagram::bind(dir.join("local.sock")).unwrap();
    service
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    for (name, tool) in [
        ("posttooluse-bash-json.json", "bash"),
        ("posttooluse-mcp-text.json", "mcp"),
    ] {
        let input = fixture(name);
        let baseline = run(&args, &input);
        assert!(!baseline.stdout.is_empty(), "{name}");
        let output = run_in(&dir, &args, &input);
        assert_eq!(output.status.code(), Some(0), "{name}");
        assert!(output.stderr.is_empty(), "{name}");
        assert_eq!(output.stdout, baseline.stdout, "{name}");

        let mut buf = [0; 1024];
        let n = service.recv(&mut buf).expect("hook sends a report");
        let report = json(&buf[..n]);
        assert_eq!(report["adapter"], "hook", "{name}");
        assert_eq!(report["tool"], tool, "{name}");
    }
}

#[test]
fn stale_socket_does_not_change_output_or_block() {
    let input = fixture("posttooluse-bash-json.json");
    let args = ["hook", "claude", "--minify-json"];
    let baseline = run(&args, &input);
    let dir = temp_dir("stale");
    // Bind and drop: the socket file stays with no listener behind it.
    drop(UnixDatagram::bind(dir.join("local.sock")).unwrap());

    let start = Instant::now();
    let output = run_in(&dir, &args, &input);
    assert!(start.elapsed() < Duration::from_secs(5));
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, baseline.stdout);
}

fn replaced(updated: Value) -> Value {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "updatedToolOutput": updated,
        }
    })
}

/// stdout holds either nothing or exactly one line with one JSON object.
fn single_output(output: &Output) -> Option<Value> {
    assert_eq!(output.status.code(), Some(0));
    if output.stdout.is_empty() {
        return None;
    }
    let text = std::str::from_utf8(&output.stdout).unwrap();
    assert_eq!(text.matches('\n').count(), 1, "{text}");
    assert!(text.ends_with('\n'), "{text}");
    let value = json(&output.stdout);
    assert!(value.is_object(), "{text}");
    Some(value)
}

#[test]
fn mcp_text_result_is_replaced() {
    let input = fixture("posttooluse-mcp-text.json");
    let output = run(&["hook", "claude", "--minify-json"], &input);
    if budget_missed(&output) {
        return;
    }
    assert_eq!(
        single_output(&output),
        Some(replaced(
            json!([{"type": "text", "text": r#"{"a":1,"b":[1,2]}"#}])
        ))
    );
}

#[test]
fn subagent_result_is_replaced_like_main_agent() {
    let mut event = json(&fixture("posttooluse-subagent-bash.json"));
    event["tool_response"]["stdout"] = "{\n  \"sub\": true\n}".into();
    let mut expected = event["tool_response"].clone();
    expected["stdout"] = r#"{"sub":true}"#.into();
    let output = run(
        &["hook", "claude", "--minify-json"],
        &serde_json::to_vec(&event).unwrap(),
    );
    if budget_missed(&output) {
        return;
    }
    assert_eq!(single_output(&output), Some(replaced(expected)));
}

#[test]
fn parallel_invocations_do_not_contaminate_each_other() {
    let dir = temp_dir("empty");
    let bash = json(&fixture("posttooluse-bash.json"));
    let subagent = json(&fixture("posttooluse-subagent-bash.json"));
    let mcp = json(&fixture("posttooluse-mcp-text.json"));
    let read = json(&fixture("posttooluse-read.json"));
    let cases: Vec<(Vec<u8>, Option<Value>)> = (0..24)
        .map(|i| {
            let pretty = format!("{{\n  \"i\": {i}\n}}");
            let compact = format!(r#"{{"i":{i}}}"#);
            let mut event = match i % 4 {
                0 => bash.clone(),
                1 => subagent.clone(),
                2 => mcp.clone(),
                _ => read.clone(),
            };
            event["tool_use_id"] = format!("toolu_{i}").into();
            let expected = match i % 4 {
                0 | 1 => {
                    event["tool_response"]["stdout"] = pretty.into();
                    let mut updated = event["tool_response"].clone();
                    updated["stdout"] = compact.into();
                    Some(replaced(updated))
                }
                2 => {
                    event["tool_response"][0]["text"] = pretty.into();
                    Some(replaced(json!([{"type": "text", "text": compact}])))
                }
                _ => {
                    event["tool_response"]["file"]["content"] = pretty.into();
                    None
                }
            };
            (serde_json::to_vec(&event).unwrap(), expected)
        })
        .collect();
    let mut children: Vec<Child> = cases
        .iter()
        .map(|_| spawn_in(&dir, &["hook", "claude", "--minify-json"]))
        .collect();
    for (child, (input, _)) in children.iter_mut().zip(&cases) {
        child.stdin.take().unwrap().write_all(input).unwrap();
    }
    // 24 processes contend for the CPU, so any job can miss the 10 ms default
    // budget and fail open. Every replacement must still be exactly its own
    // case's, never another case's content.
    for (i, (child, (_, expected))) in children.into_iter().zip(&cases).enumerate() {
        let output = child.wait_with_output().unwrap();
        let actual = single_output(&output);
        if budget_missed(&output) {
            continue;
        }
        assert_eq!(&actual, expected, "case {i}");
    }
}

#[test]
fn conflicting_hook_is_reported_and_output_left_unchanged() {
    let dir = temp_dir("conflict");
    let settings = json!({"hooks": {"PostToolUse": [
        {"matcher": "Edit|Bash", "hooks": [{"type": "command", "command": "jq ."}]}
    ]}});
    std::fs::write(dir.join("settings.json"), settings.to_string()).unwrap();
    let output = run_in(
        &dir,
        &["hook", "claude", "--minify-json"],
        &fixture("posttooluse-bash-json.json"),
    );
    assert_eq!(single_output(&output), None);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    // Compression runs before conflict detection, so a budget miss replaces the conflict diagnostic.
    if stderr == "deslag: compression budget_exceeded; tool output unchanged\n" {
        return;
    }
    assert!(stderr.contains("settings.json"), "{stderr}");
}
