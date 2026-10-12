use std::io::Write;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn run(args: &[&str], stdin: &[u8]) -> Output {
    // No service lives here, so no test reports to a real state dir.
    run_in(Path::new("/nonexistent/deslag-test"), args, stdin)
}

fn run_in(state_dir: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_deslag"))
        .args(args)
        .env("DESLAG_STATE_DIR", state_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn deslag");
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
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

fn json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap()
}

#[test]
fn minify_flag_replaces_pretty_json_bash_stdout() {
    let input = fixture("posttooluse-bash-json.json");
    let output = run(&["hook", "claude", "--minify-json"], &input);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
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
    assert_eq!(again.stdout, output.stdout);
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
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn without_flag_pretty_json_is_untouched() {
    let output = run(&["hook", "claude"], &fixture("posttooluse-bash-json.json"));
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

/// A fresh short directory under /tmp, removed on drop: macOS caps socket
/// paths at 104 bytes.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_dir(name: &str) -> TempDir {
    let dir = PathBuf::from(format!("/tmp/dsl-hook-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(dir)
}

#[test]
fn live_service_gets_a_hook_report_and_output_is_unchanged() {
    let input = fixture("posttooluse-bash-json.json");
    let args = ["hook", "claude", "--minify-json"];
    let baseline = run(&args, &input);
    let dir = temp_dir("live");
    let service = UnixDatagram::bind(dir.0.join("local.sock")).unwrap();
    service
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let output = run_in(&dir.0, &args, &input);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, baseline.stdout);

    let mut buf = [0; 1024];
    let n = service.recv(&mut buf).expect("hook sends a report");
    let report = json(&buf[..n]);
    assert_eq!(report["adapter"], "hook");
    assert_eq!(report["tool"], "bash");
}

#[test]
fn stale_socket_does_not_change_output_or_block() {
    let input = fixture("posttooluse-bash-json.json");
    let args = ["hook", "claude", "--minify-json"];
    let baseline = run(&args, &input);
    let dir = temp_dir("stale");
    // Bind and drop: the socket file stays with no listener behind it.
    drop(UnixDatagram::bind(dir.0.join("local.sock")).unwrap());

    let start = Instant::now();
    let output = run_in(&dir.0, &args, &input);
    assert!(start.elapsed() < Duration::from_secs(5));
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, baseline.stdout);
}
