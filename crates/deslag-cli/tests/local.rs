use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// A fresh short state dir under /tmp: macOS caps socket paths at 104 bytes.
fn state_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/dsl-it-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn command(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_deslag"));
    cmd.args(args)
        .env("DESLAG_STATE_DIR", dir)
        // Keep the developer's real Claude Code settings out of hook conflict checks.
        .env("CLAUDE_CONFIG_DIR", dir)
        .env("CLAUDE_PROJECT_DIR", dir)
        .env("HOME", dir);
    cmd
}

fn deslag(dir: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut child = command(dir, args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn deslag");
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(output: &Output) -> String {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn hook_input() -> Vec<u8> {
    let path = format!(
        "{}/../../tests/fixtures/claude/posttooluse-bash-json.json",
        env!("CARGO_MANIFEST_DIR")
    );
    fs::read(path).unwrap()
}

/// Removes the state dir on drop.
struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Sends one datagram, retrying while macOS returns ENOBUFS for the service's
/// small receive queue.
fn send(client: &UnixDatagram, message: &[u8], dir: &Path) {
    let mut tries = 0;
    while let Err(e) = client.send_to(message, dir.join("local.sock")) {
        tries += 1;
        assert!(tries < 200, "{e}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// Sends a control message from a bound client socket and returns the reply.
fn request(dir: &Path, message: &[u8]) -> Vec<u8> {
    let path = dir.join("test.sock");
    let _ = fs::remove_file(&path);
    let client = UnixDatagram::bind(&path).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    send(&client, message, dir);
    let mut buf = [0; 512];
    let n = client.recv(&mut buf).unwrap();
    let _ = fs::remove_file(&path);
    buf[..n].to_vec()
}

/// A foreground `deslag local run`. Kills the service on drop, so a failing
/// assertion never orphans it.
struct Service(Child, PathBuf);

impl Service {
    /// Sends the `stop` datagram and waits for a clean exit.
    fn stop(mut self) {
        assert_eq!(request(&self.1, b"stop"), b"ok");
        assert!(self.0.wait().unwrap().success());
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

/// Spawns `deslag local run` and waits until it answers `ping`.
fn run(dir: &Path) -> Service {
    let child = command(dir, &["local", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn deslag local run");
    // Guard first: a failed wait below kills the child on unwind.
    let mut service = Service(child, dir.to_path_buf());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !dir.join("local.sock").exists() {
        assert!(service.0.try_wait().unwrap().is_none(), "service exited");
        assert!(Instant::now() < deadline, "service did not bind");
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(request(dir, b"ping"), b"ok");
    service
}

#[test]
fn service_persists_buckets_across_restarts() {
    let dir = state_dir("life");
    let _cleanup = RemoveOnDrop(dir.clone());
    let hook = ["hook", "claude", "--minify-json"];
    let expected = deslag(&state_dir("none"), &hook, &hook_input()).stdout;
    assert!(!expected.is_empty());

    for round in 1..=2 {
        let service = run(&dir);
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join("local.sock")), 0o600);

        let output = deslag(&dir, &hook, &hook_input());
        assert_eq!(output.stdout, expected, "service changes no hook output");
        assert!(output.stderr.is_empty());

        service.stop();
        assert!(!dir.join("local.sock").exists());
        let stats = dir.join("stats.jsonl");
        assert_eq!(mode(&stats), 0o600);
        let text = fs::read_to_string(&stats).unwrap();
        for forbidden in [
            "00000000-0000",
            "/home/user",
            "Bash",
            "toolu_",
            "deslag\\\"",
        ] {
            assert!(!text.contains(forbidden), "{forbidden} in {text}");
        }
        let reports: Vec<serde_json::Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .filter(|l: &serde_json::Value| l["adapter"] == "hook")
            .collect();
        assert_eq!(reports.len(), round, "{text}");
        assert!(
            reports
                .iter()
                .all(|r| r["count"] == 1 && r["outcome"] == "changed" && r["tool"] == "bash")
        );
    }
}

#[test]
fn flush_writes_the_open_hour_and_keeps_the_service_running() {
    let dir = state_dir("flush");
    let _cleanup = RemoveOnDrop(dir.clone());
    let service = run(&dir);
    send(
        &UnixDatagram::unbound().unwrap(),
        report().to_string().as_bytes(),
        &dir,
    );
    assert_eq!(request(&dir, b"flush"), b"ok");
    let text = fs::read_to_string(dir.join("stats.jsonl")).unwrap();
    assert!(text.contains(r#""adapter":"hook""#), "{text}");
    assert_eq!(request(&dir, b"ping"), b"ok");
    service.stop();
}

#[test]
fn hook_output_is_unchanged_with_a_stale_socket() {
    let dir = state_dir("stale");
    fs::create_dir_all(&dir).unwrap();
    drop(UnixDatagram::bind(dir.join("local.sock")).unwrap());
    let hook = ["hook", "claude", "--minify-json"];
    let with_stale = deslag(&dir, &hook, &hook_input());
    let without = deslag(&state_dir("absent"), &hook, &hook_input());
    assert_eq!(with_stale.status.code(), Some(0));
    assert!(with_stale.stderr.is_empty());
    assert_eq!(with_stale.stdout, without.stdout);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_replaces_a_stale_socket() {
    let dir = state_dir("stalerun");
    let _cleanup = RemoveOnDrop(dir.clone());
    fs::create_dir_all(&dir).unwrap();
    drop(UnixDatagram::bind(dir.join("local.sock")).unwrap());
    // `run` waits for a `ping` answer, which the stale socket never gives.
    run(&dir).stop();
}

#[test]
fn bad_local_arguments_print_usage_and_exit_64() {
    let dir = state_dir("usage");
    for args in [
        &["local"][..],
        &["local", "restart"],
        &["local", "run", "--retention-days", "0"],
        &["local", "run", "--retention-days", "x"],
        &["local", "run", "extra"],
    ] {
        let output = deslag(&dir, args, b"");
        assert_eq!(output.status.code(), Some(64), "{args:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage: deslag local"));
    }
}

#[test]
fn run_survives_a_corrupt_stats_file_and_a_loose_state_dir() {
    let dir = state_dir("corrupt");
    let _cleanup = RemoveOnDrop(dir.clone());
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(dir.join("stats.jsonl"), b"\xff\n").unwrap();
    let service = run(&dir);
    assert_eq!(mode(&dir), 0o700);
    stdout(&deslag(
        &dir,
        &["hook", "claude", "--minify-json"],
        &hook_input(),
    ));
    service.stop();
    let text = fs::read(dir.join("stats.jsonl")).unwrap();
    let text = String::from_utf8_lossy(&text);
    assert!(text.contains(r#""adapter":"hook""#), "{text}");
}

#[test]
fn a_second_run_reports_already_running() {
    let dir = state_dir("second");
    let _cleanup = RemoveOnDrop(dir.clone());
    let service = run(&dir);
    let mut child = command(&dir, &["local", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn second deslag local run");
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("second run did not exit");
        }
        thread::sleep(Duration::from_millis(20));
    }
    let second = child.wait_with_output().unwrap();
    assert_eq!(second.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&second.stderr).contains("already running"));
    assert_eq!(request(&dir, b"ping"), b"ok");
    service.stop();
}

#[test]
fn hook_passes_through_when_the_state_dir_is_a_file() {
    let dir = state_dir("file");
    fs::write(&dir, b"").unwrap();
    let hook = ["hook", "claude", "--minify-json"];
    let output = deslag(&dir, &hook, &hook_input());
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        deslag(&state_dir("nofile"), &hook, &hook_input()).stdout
    );
    let _ = fs::remove_file(&dir);
}

fn report() -> serde_json::Value {
    serde_json::json!({"adapter":"hook","tool":"bash","transform":"json-minify",
        "outcome":"changed","reason":null,"original_bytes":1,"candidate_bytes":null,
        "emitted_bytes":1,"tokens":{"method":"estimate","tokenizer":"deslag-estimate-v1",
        "original":1,"candidate":null,"emitted":1},"dropped":0})
}

/// Runs a service, sends `messages` as datagrams, stops it and returns the
/// report lines and the summed service drops from `stats.jsonl`.
fn record(name: &str, messages: &[Vec<u8>]) -> (Vec<serde_json::Value>, u64) {
    let dir = state_dir(name);
    let _cleanup = RemoveOnDrop(dir.clone());
    let service = run(&dir);
    let client = UnixDatagram::unbound().unwrap();
    for message in messages {
        send(&client, message, &dir);
    }
    service.stop();
    let text = fs::read_to_string(dir.join("stats.jsonl")).unwrap();
    let (service, reports): (Vec<serde_json::Value>, _) = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .partition(|l: &serde_json::Value| l["adapter"].is_null());
    let dropped = service.iter().map(|l| l["dropped"].as_u64().unwrap()).sum();
    (reports, dropped)
}

#[test]
fn datagrams_over_the_message_cap_count_as_drops() {
    let messages: Vec<Vec<u8>> = [512, 513, 600]
        .into_iter()
        .map(|len| {
            let mut message = report().to_string().into_bytes();
            message.resize(len, b' ');
            message
        })
        .collect();
    let (reports, dropped) = record("cap", &messages);
    assert_eq!(dropped, 2);
    assert_eq!(reports.len(), 1, "a datagram at the cap is a report");
    assert_eq!(reports[0]["count"], 1);
}

#[test]
fn invalid_or_unlisted_reports_count_as_drops() {
    let mut messages = vec![b"not json".to_vec(), b"{}".to_vec()];
    for (field, value) in [
        ("tool", serde_json::json!("Bash")),
        ("transform", serde_json::json!("/home/user/x")),
        ("adapter", serde_json::json!("other")),
        ("reason", serde_json::json!("session-123")),
        ("original_bytes", serde_json::json!(-1)),
        (
            "tokens",
            serde_json::json!({"method":"estimate","tokenizer":"/etc/passwd","original":1,"candidate":null,"emitted":1}),
        ),
        (
            "tokens",
            serde_json::json!({"method":"estimate","tokenizer":"0b6f1a2c-3d4e-4f50-8a1b-2c3d4e5f6a7b","original":1,"candidate":null,"emitted":1}),
        ),
        // Each value is allowed alone; only the pair list decides.
        (
            "tokens",
            serde_json::json!({"method":"exact","tokenizer":"deslag-estimate-v1","original":1,"candidate":null,"emitted":1}),
        ),
        (
            "tokens",
            serde_json::json!({"method":"guess","tokenizer":"deslag-estimate-v1","original":1,"candidate":null,"emitted":1}),
        ),
    ] {
        let mut v = report();
        v[field] = value;
        messages.push(v.to_string().into_bytes());
    }
    let (reports, dropped) = record("invalid", &messages);
    assert!(reports.is_empty(), "{reports:?}");
    assert_eq!(dropped, messages.len() as u64);
}

#[test]
fn full_range_u64_sizes_and_tokens_parse() {
    // serde_json's arbitrary_precision is on workspace-wide; as_u64 must still accept u64::MAX.
    let mut v = report();
    v["original_bytes"] = serde_json::json!(u64::MAX);
    v["tokens"]["original"] = serde_json::json!(u64::MAX);
    let mut messages = vec![v.to_string().into_bytes()];
    for bad in [
        serde_json::json!(1.0),
        serde_json::json!(-1),
        serde_json::json!(u64::MAX as f64),
    ] {
        v["original_bytes"] = bad;
        messages.push(v.to_string().into_bytes());
    }
    let (reports, dropped) = record("u64", &messages);
    assert_eq!(dropped, 3);
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0]["original_bytes"].as_u64(), Some(u64::MAX));
    let tokens = &reports[0]["tokens"]["estimate:deslag-estimate-v1"];
    assert_eq!(tokens["original"].as_u64(), Some(u64::MAX));
}

#[test]
fn a_report_without_tokens_is_aggregated() {
    let mut v = report();
    v["tokens"] = serde_json::Value::Null;
    let (reports, dropped) = record("notokens", &[v.to_string().into_bytes()]);
    assert_eq!(dropped, 0);
    assert_eq!(reports.len(), 1, "{reports:?}");
}
