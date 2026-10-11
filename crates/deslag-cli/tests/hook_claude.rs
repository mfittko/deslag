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
    let output = run(&["hook", "claude"], b"{not json");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn unknown_arguments_print_usage_and_exit_2() {
    for args in [
        &[][..],
        &["hook"],
        &["hook", "codex"],
        &["hook", "claude", "x"],
    ] {
        let output = run(args, b"");
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage"));
    }
}
