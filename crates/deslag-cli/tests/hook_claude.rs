use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use deslag_proxy::session::{Owner, write_owner};

/// A fresh state directory with `owner` written, if any.
fn state(owner: Option<Owner>) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "deslag-hook-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    if let Some(owner) = owner {
        write_owner(&dir, owner).unwrap();
    }
    dir
}

/// The binary with every state-dir variable cleared, then `env` applied.
fn run_env(args: &[&str], stdin: &[u8], env: &[(&str, &PathBuf)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_deslag"));
    for var in ["DESLAG_STATE_DIR", "XDG_STATE_HOME", "HOME"] {
        cmd.env_remove(var);
    }
    let mut child = cmd
        .envs(env.iter().copied())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn deslag");
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

/// Runs with a state directory whose owner is the hook.
fn run(args: &[&str], stdin: &[u8]) -> Output {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    let dir = DIR.get_or_init(|| state(Some(Owner::Hook)));
    run_env(args, stdin, &[("DESLAG_STATE_DIR", dir)])
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

#[test]
fn minify_flag_replaces_only_when_the_hook_owns_compression() {
    let input = fixture("posttooluse-bash-json.json");
    let args = ["hook", "claude", "--minify-json"];
    for owner in [Some(Owner::Proxy), Some(Owner::None), None] {
        let dir = state(owner);
        let output = run_env(&args, &input, &[("DESLAG_STATE_DIR", &dir)]);
        assert_eq!(output.status.code(), Some(0), "{owner:?}");
        assert!(output.stdout.is_empty(), "{owner:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }
    let unknown = state(None);
    std::fs::write(unknown.join("owner"), "hooks\n").unwrap();
    let output = run_env(&args, &input, &[("DESLAG_STATE_DIR", &unknown)]);
    assert!(output.stdout.is_empty(), "unknown owner");
    assert!(
        run_env(&args, &input, &[]).stdout.is_empty(),
        "no state dir"
    );

    // XDG_STATE_HOME/deslag, then HOME/.local/state/deslag.
    let xdg = state(None);
    write_owner(&xdg.join("deslag"), Owner::Hook).unwrap();
    assert!(
        !run_env(&args, &input, &[("XDG_STATE_HOME", &xdg)])
            .stdout
            .is_empty()
    );
    let home = state(None);
    write_owner(&home.join(".local/state/deslag"), Owner::Hook).unwrap();
    assert!(!run_env(&args, &input, &[("HOME", &home)]).stdout.is_empty());

    // Relative values are skipped, so the cwd never selects the owner file.
    let relative = PathBuf::from("relative-state");
    let env = [
        ("DESLAG_STATE_DIR", &relative),
        ("XDG_STATE_HOME", &relative),
        ("HOME", &home),
    ];
    assert!(!run_env(&args, &input, &env).stdout.is_empty());
    for dir in [unknown, xdg, home] {
        std::fs::remove_dir_all(dir).unwrap();
    }
}
