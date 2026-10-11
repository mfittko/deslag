use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_deslag"))
        .args(args)
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
        let output = run(&["hook", "claude"], input);
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
    }
}

#[test]
fn unknown_arguments_print_usage_and_exit_64() {
    for args in [
        &[][..],
        &["hook"],
        &["hook", "codex"],
        &["hook", "claude", "x"],
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
