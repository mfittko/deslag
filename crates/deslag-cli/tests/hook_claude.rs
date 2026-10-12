use std::io::Write;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use deslag_proxy::session::{Owner, write_owner};
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

/// A fresh state directory with `owner` written, if any.
fn state(owner: Option<Owner>) -> PathBuf {
    let dir = temp_dir("state");
    if let Some(owner) = owner {
        write_owner(&dir, owner).unwrap();
    }
    dir
}

/// A shared state directory whose owner is the hook.
fn hook_state() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| state(Some(Owner::Hook)))
}

/// Spawns the hook with config and project dirs at `dir`, so the developer's real
/// Claude Code settings never apply. Every state-dir variable is cleared, so the
/// real owner file never applies either; then `env` is applied.
fn spawn_in(dir: &Path, args: &[&str], env: &[(&str, &Path)]) -> Child {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_deslag"));
    for var in ["DESLAG_STATE_DIR", "XDG_STATE_HOME", "HOME"] {
        cmd.env_remove(var);
    }
    cmd.args(args)
        .env("CLAUDE_CONFIG_DIR", dir)
        .env("CLAUDE_PROJECT_DIR", dir)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn deslag")
}

fn run_in(dir: &Path, args: &[&str], stdin: &[u8], env: &[(&str, &Path)]) -> Output {
    let mut child = spawn_in(dir, args, env);
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

/// Runs with no Claude settings and the given state-dir variables.
fn run_env(args: &[&str], stdin: &[u8], env: &[(&str, &Path)]) -> Output {
    run_in(&temp_dir("empty"), args, stdin, env)
}

/// Runs with no Claude settings and a state directory whose owner is the hook.
fn run(args: &[&str], stdin: &[u8]) -> Output {
    run_env(args, stdin, &[("DESLAG_STATE_DIR", hook_state())])
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
    let mcp = fixture("posttooluse-mcp-text.json");
    let args = ["hook", "claude", "--minify-json"];
    // The gate covers every routed field: Bash stdout and MCP text blocks alike.
    for owner in [Some(Owner::Proxy), Some(Owner::None), None] {
        let dir = state(owner);
        for event in [&input, &mcp] {
            let output = run_env(&args, event, &[("DESLAG_STATE_DIR", &dir)]);
            assert_eq!(output.status.code(), Some(0), "{owner:?}");
            assert!(output.stdout.is_empty(), "{owner:?}");
            assert!(output.stderr.is_empty(), "{owner:?}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    assert!(!run(&args, &mcp).stdout.is_empty(), "mcp with hook owner");
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
    let relative = Path::new("relative-state");
    let env = [
        ("DESLAG_STATE_DIR", relative),
        ("XDG_STATE_HOME", relative),
        ("HOME", &home),
    ];
    assert!(!run_env(&args, &input, &env).stdout.is_empty());
    for dir in [unknown, xdg, home] {
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn live_service_gets_a_hook_report_and_output_is_unchanged() {
    let args = ["hook", "claude", "--minify-json"];
    // The hook reports only results it compresses, so it must own the session.
    let dir = state(Some(Owner::Hook));
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
        let output = run_env(&args, &input, &[("DESLAG_STATE_DIR", &dir)]);
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
    let dir = state(Some(Owner::Hook));
    // Bind and drop: the socket file stays with no listener behind it.
    drop(UnixDatagram::bind(dir.join("local.sock")).unwrap());

    let start = Instant::now();
    let output = run_env(&args, &input, &[("DESLAG_STATE_DIR", &dir)]);
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
    assert!(output.stderr.is_empty());
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
    assert!(output.stderr.is_empty());
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
        .map(|_| {
            spawn_in(
                &dir,
                &["hook", "claude", "--minify-json"],
                &[("DESLAG_STATE_DIR", hook_state())],
            )
        })
        .collect();
    for (child, (input, _)) in children.iter_mut().zip(&cases) {
        child.stdin.take().unwrap().write_all(input).unwrap();
    }
    for (i, (child, (_, expected))) in children.into_iter().zip(&cases).enumerate() {
        let output = child.wait_with_output().unwrap();
        assert!(output.stderr.is_empty(), "case {i}");
        assert_eq!(&single_output(&output), expected, "case {i}");
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
        &[("DESLAG_STATE_DIR", hook_state())],
    );
    assert_eq!(single_output(&output), None);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.contains("settings.json"), "{stderr}");
}
