use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use deslag_proxy::session::{Owner, write_owner};

/// A fresh short state dir under /tmp: macOS caps socket paths at 104 bytes.
fn state_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/dsl-it-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

/// A fresh state dir whose owner file says the hook owns compression.
fn hook_state_dir(name: &str) -> PathBuf {
    let dir = state_dir(name);
    write_owner(&dir, Owner::Hook).unwrap();
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

/// Stops a `deslag local start` service on drop, so a failing assertion never
/// orphans it.
struct StopOnDrop(PathBuf);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let _ = deslag(&self.0, &["local", "stop"], b"");
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

/// Sends one `ping` without retries and reports whether the service answered `ok`.
fn pinged(dir: &Path) -> bool {
    let path = dir.join("ping.sock");
    let _ = fs::remove_file(&path);
    let Ok(client) = UnixDatagram::bind(&path) else {
        return false;
    };
    client
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let mut buf = [0; 16];
    let ok = client.send_to(b"ping", dir.join("local.sock")).is_ok()
        && client.recv(&mut buf).is_ok_and(|n| &buf[..n] == b"ok");
    let _ = fs::remove_file(&path);
    ok
}

/// Spawns `deslag local run` and waits until it answers `ping`.
fn run(dir: &Path) -> Service {
    run_with(dir, &[])
}

/// Spawns `deslag local run <args>` and waits until it answers `ping`. A
/// stale socket never answers, so it cannot end the wait early.
fn run_with(dir: &Path, args: &[&str]) -> Service {
    let child = command(dir, &[&["local", "run"][..], args].concat())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn deslag local run");
    // Guard first: a failed wait below kills the child on unwind.
    let mut service = Service(child, dir.to_path_buf());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !pinged(dir) {
        assert!(service.0.try_wait().unwrap().is_none(), "service exited");
        assert!(Instant::now() < deadline, "service did not answer ping");
        thread::sleep(Duration::from_millis(20));
    }
    service
}

#[test]
fn service_persists_buckets_across_restarts() {
    let dir = hook_state_dir("life");
    let _cleanup = RemoveOnDrop(dir.clone());
    let hook = ["hook", "claude", "--minify-json"];
    let plain = hook_state_dir("none");
    let _plain_cleanup = RemoveOnDrop(plain.clone());
    let expected = deslag(&plain, &hook, &hook_input()).stdout;
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
    let dir = hook_state_dir("stale");
    fs::create_dir_all(&dir).unwrap();
    drop(UnixDatagram::bind(dir.join("local.sock")).unwrap());
    let hook = ["hook", "claude", "--minify-json"];
    let with_stale = deslag(&dir, &hook, &hook_input());
    let absent = hook_state_dir("absent");
    let without = deslag(&absent, &hook, &hook_input());
    assert!(!without.stdout.is_empty());
    assert_eq!(with_stale.status.code(), Some(0));
    assert!(with_stale.stderr.is_empty());
    assert_eq!(with_stale.stdout, without.stdout);
    assert!(stdout(&deslag(&dir, &["local", "status"], b"")).contains("not running"));
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&absent);
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
fn a_failed_flush_or_stop_replies_and_keeps_the_service_running() {
    let dir = state_dir("flushfail");
    let _cleanup = RemoveOnDrop(dir.clone());
    // A directory in place of stats.jsonl makes every write fail.
    fs::create_dir_all(dir.join("stats.jsonl")).unwrap();
    let service = run(&dir);
    send(
        &UnixDatagram::unbound().unwrap(),
        report().to_string().as_bytes(),
        &dir,
    );
    for message in [&b"flush"[..], b"stop"] {
        let reply = String::from_utf8(request(&dir, message)).unwrap();
        assert!(reply.starts_with("flush failed: "), "{reply}");
        assert_eq!(request(&dir, b"ping"), b"ok");
    }
    fs::remove_dir(dir.join("stats.jsonl")).unwrap();
    service.stop();
    let text = fs::read_to_string(dir.join("stats.jsonl")).unwrap();
    assert!(text.contains(r#""adapter":"hook""#), "buckets kept: {text}");
}

#[test]
fn run_applies_the_retention_days_argument() {
    let dir = state_dir("retention");
    let _cleanup = RemoveOnDrop(dir.clone());
    fs::create_dir_all(&dir).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let hour = now - now % 3600;
    // Canonical service lines: rotation drops lines it cannot parse.
    let service_line = |hour: u64, dropped: u64| {
        format!(
            r#"{{"adapter":null,"candidate_bytes":null,"count":0,"dropped":{dropped},"emitted_bytes":0,"hour":{hour},"original_bytes":0,"outcome":null,"reason":null,"tokens":{{}},"tool":null,"transform":null,"v":1}}"#
        )
    };
    let old = service_line(hour - 2 * 24 * 3600, 1);
    let recent = service_line(hour, 2);
    let stats = dir.join("stats.jsonl");
    fs::write(&stats, format!("{old}\n{recent}\n")).unwrap();
    fs::set_permissions(&stats, fs::Permissions::from_mode(0o600)).unwrap();
    // The default 90-day retention keeps both lines; only the argument drops `old`.
    run_with(&dir, &["--retention-days", "1"]).stop();
    let text = fs::read_to_string(&stats).unwrap();
    assert!(!text.contains(&old), "{text}");
    assert!(text.contains(&recent), "{text}");
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
        &["local", "stop", "--retention-days", "3"],
        &["local", "start", "--retention-days", "x"],
    ] {
        let output = deslag(&dir, args, b"");
        assert_eq!(output.status.code(), Some(64), "{args:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage: deslag local"));
    }
}

#[test]
fn run_survives_a_corrupt_stats_file_and_a_loose_state_dir() {
    let dir = hook_state_dir("corrupt");
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
        ("emitted_bytes", serde_json::json!(u64::from(u32::MAX) + 1)),
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
    // serde_json's arbitrary_precision is on workspace-wide; as_u64 must still accept
    // u64::MAX. Sizes stop at the u32::MAX cap that parse_report applies.
    let mut v = report();
    v["original_bytes"] = serde_json::json!(u32::MAX);
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
    assert_eq!(
        reports[0]["original_bytes"].as_u64(),
        Some(u64::from(u32::MAX))
    );
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

#[test]
fn start_status_and_stop_drive_the_service_lifecycle() {
    let dir = hook_state_dir("cli");
    let _guard = StopOnDrop(dir.clone());
    assert!(stdout(&deslag(&dir, &["local", "status"], b"")).contains("not running"));
    for round in 1..=2 {
        assert!(stdout(&deslag(&dir, &["local", "start"], b"")).contains("started"));
        assert!(stdout(&deslag(&dir, &["local", "start"], b"")).contains("already running"));
        assert!(stdout(&deslag(&dir, &["local", "status"], b"")).contains(": running"));
        stdout(&deslag(
            &dir,
            &["hook", "claude", "--minify-json"],
            &hook_input(),
        ));
        assert!(stdout(&deslag(&dir, &["local", "stop"], b"")).contains("stopped"));
        assert!(!dir.join("local.sock").exists());
        let text = fs::read_to_string(dir.join("stats.jsonl")).unwrap();
        assert_eq!(text.matches(r#""adapter":"hook""#).count(), round, "{text}");
    }
    assert!(stdout(&deslag(&dir, &["local", "stop"], b"")).contains("not running"));
}

#[test]
fn start_with_retention_on_a_running_service_says_it_keeps_its_retention() {
    let dir = state_dir("keep");
    let _guard = StopOnDrop(dir.clone());
    assert!(stdout(&deslag(&dir, &["local", "start"], b"")).contains("started"));
    let again = stdout(&deslag(
        &dir,
        &["local", "start", "--retention-days", "7"],
        b"",
    ));
    assert!(
        again.contains("already running; it keeps its retention"),
        "{again}"
    );
}

#[test]
fn a_silent_socket_is_not_responding_and_no_state_dir_is_not_running() {
    let dir = state_dir("silent");
    fs::create_dir_all(&dir).unwrap();
    let _silent = UnixDatagram::bind(dir.join("local.sock")).unwrap();
    // `start` must not spawn a second service next to a busy one.
    for command in ["status", "stop", "start"] {
        let output = deslag(&dir, &["local", command], b"");
        assert_eq!(output.status.code(), Some(2), "{command}: {output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("not responding"));
    }
    let _ = fs::remove_dir_all(&dir);

    for command in ["status", "stop"] {
        let output = Command::new(env!("CARGO_BIN_EXE_deslag"))
            .args(["local", command])
            .env_remove("DESLAG_STATE_DIR")
            .env_remove("XDG_STATE_HOME")
            .env_remove("HOME")
            .output()
            .unwrap();
        assert!(stdout(&output).contains("not running"), "{command}");
    }
}

#[test]
fn a_failed_client_bind_is_an_error_not_not_running() {
    // A read-only state dir: the client cannot bind its ctl socket.
    let dir = state_dir("rodir");
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
    // A ctl socket path past sun_path (104 bytes on macOS, 108 on Linux).
    let long = dir.with_file_name(format!("dsl-it-{}-{}", std::process::id(), "x".repeat(100)));
    let _ = fs::remove_dir_all(&long);
    fs::create_dir_all(&long).unwrap();
    let mut cases = vec![("long path", &long)];
    // Root ignores 0o500: when a probe file can be created, the bind would
    // succeed, so only the read-only case is skipped.
    let probe = dir.join("probe");
    if fs::File::create(&probe).is_ok() {
        let _ = fs::remove_file(&probe);
    } else {
        cases.push(("read-only", &dir));
    }
    for (case, path) in cases {
        // `start` must not spawn a service it cannot reach.
        for command in ["status", "stop", "start"] {
            let output = deslag(path, &["local", command], b"");
            assert_eq!(
                output.status.code(),
                Some(2),
                "{case} {command}: {output:?}"
            );
            assert!(String::from_utf8_lossy(&output.stderr).contains("control failed"));
        }
    }
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&long);
}

/// Sends one raw request to the HTTP bridge and returns its status line.
fn http(dir: &Path, request: &[u8]) -> String {
    let mut stream = UnixStream::connect(dir.join("local-http.sock")).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(request).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    assert!(reply.contains("Connection: close"), "{reply}");
    reply.lines().next().unwrap().to_string()
}

/// Posts one usage record for request `index` of `session`, as the usage module does.
fn post_usage(dir: &Path, session: &str, index: u64) -> String {
    let body = serde_json::json!({"kind":"usage","session":session,"turn":"turn-1","index":index,
        "model":"claude-haiku","usage":{"input_tokens":100 + index,"output_tokens":5,
        "cache_read_input_tokens":0,"cache_creation_input_tokens":7},"dropped":index})
    .to_string();
    http(dir, &usage_request(&body))
}

fn usage_request(body: &str) -> Vec<u8> {
    format!(
        "POST /usage HTTP/1.1\r\nhost: deslag\r\ncontent-type: application/json\r\n\
         content-length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn requests(dir: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(dir.join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

const SESSION: &str = "00000000-0000-4000-8000-000000000001";

#[test]
fn usage_records_take_the_hook_reports_of_their_session() {
    let dir = hook_state_dir("usage");
    let _cleanup = RemoveOnDrop(dir.clone());
    let service = run(&dir);
    assert_eq!(mode(&dir.join("local-http.sock")), 0o600);
    assert_eq!(post_usage(&dir, SESSION, 0), "HTTP/1.1 204 No Content");
    // The fixture event carries SESSION; a report without a session is not attributed.
    stdout(&deslag(
        &dir,
        &["hook", "claude", "--minify-json"],
        &hook_input(),
    ));
    send(
        &UnixDatagram::unbound().unwrap(),
        report().to_string().as_bytes(),
        &dir,
    );
    assert_eq!(
        post_usage(&dir, "other-session", 0),
        "HTTP/1.1 204 No Content"
    );
    assert_eq!(post_usage(&dir, SESSION, 1), "HTTP/1.1 204 No Content");

    // Malformed requests: 400, never forwarded. An invalid record is a service drop.
    assert_eq!(
        http(&dir, b"GET /usage HTTP/1.1\r\n\r\n"),
        "HTTP/1.1 400 Bad Request"
    );
    assert_eq!(
        http(&dir, b"POST /usage HTTP/1.1\r\n\r\n"),
        "HTTP/1.1 400 Bad Request"
    );
    let big = "x".repeat(513);
    assert_eq!(http(&dir, &usage_request(&big)), "HTTP/1.1 400 Bad Request");
    // Only a JSON object passes, so control messages cannot reach the service.
    for control in ["stop", "flush"] {
        assert_eq!(
            http(&dir, &usage_request(control)),
            "HTTP/1.1 400 Bad Request"
        );
    }
    let partial = r#"{"kind":"usage","session":"s","turn":"t","index":0,"model":"m","usage":{"input_tokens":1},"dropped":0}"#;
    assert_eq!(
        http(&dir, &usage_request(partial)),
        "HTTP/1.1 204 No Content"
    );
    service.stop();
    assert!(!dir.join("local-http.sock").exists());

    assert_eq!(mode(&dir.join("requests.jsonl")), 0o600);
    let lines = requests(&dir);
    let take = |l: &serde_json::Value| {
        (
            l["session"].clone(),
            l["index"].clone(),
            l["compression_reports"].clone(),
        )
    };
    let got: Vec<_> = lines.iter().map(take).collect();
    assert_eq!(
        got,
        [
            (SESSION.into(), 0.into(), 0.into()),
            ("other-session".into(), 0.into(), 0.into()),
            (SESSION.into(), 1.into(), 1.into()),
        ]
    );
    assert!(
        lines[2]["estimated_removed_tokens"].as_u64().unwrap() > 0,
        "{lines:?}"
    );
    let stats = fs::read_to_string(dir.join("stats.jsonl")).unwrap();
    assert!(
        !stats.contains(SESSION),
        "buckets hold no session ids: {stats}"
    );

    let view = stdout(&deslag(
        &dir,
        &["stats", "--json", "--session", SESSION],
        b"",
    ));
    let view: serde_json::Value = serde_json::from_str(&view).unwrap();
    let listed = view["requests"].as_array().unwrap();
    assert_eq!(listed.len(), 2, "{view}");
    assert_eq!(listed[1]["usage"]["input_tokens"], 101);
    assert_eq!(
        listed[1]["estimated_removed_tokens"],
        lines[2]["estimated_removed_tokens"]
    );
    assert_eq!(view["totals"]["input_tokens"], 201);
    let all = stdout(&deslag(&dir, &["stats", "--json"], b""));
    let all: serde_json::Value = serde_json::from_str(&all).unwrap();
    assert_eq!(all["totals"]["usage"]["requests"], 3);
    // Record drops (0 + 0 + 1) plus the partial record's service drop.
    assert_eq!(all["totals"]["dropped"], 2, "{all}");
}

#[test]
fn the_bridge_answers_a_session_read_with_the_stats_document() {
    let dir = state_dir("session");
    let _cleanup = RemoveOnDrop(dir.clone());
    let service = run(&dir);
    assert_eq!(post_usage(&dir, SESSION, 0), "HTTP/1.1 204 No Content");
    let mut stream = UnixStream::connect(dir.join("local-http.sock")).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET /session/{SESSION} HTTP/1.1\r\nhost: deslag\r\n\r\n"
    )
    .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    let (head, body) = reply.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "{reply}");
    assert!(head.contains("Content-Type: application/json"), "{head}");
    assert!(
        head.contains(&format!("Content-Length: {}", body.len())),
        "{head}"
    );
    let doc: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(doc["schema_version"], 1);
    assert_eq!(doc["session"], SESSION);
    let listed = doc["requests"].as_array().unwrap();
    assert_eq!(listed.len(), 1, "{doc}");
    assert_eq!(listed[0]["usage"]["input_tokens"], 100);
    assert_eq!(listed[0]["estimated_removed_tokens"], 0);
    // Same document as the CLI, except the clock.
    let cli = stdout(&deslag(
        &dir,
        &["stats", "--json", "--session", SESSION],
        b"",
    ));
    let mut cli: serde_json::Value = serde_json::from_str(&cli).unwrap();
    cli["window"] = doc["window"].clone();
    assert_eq!(cli, doc);

    for bad in ["a%20b", "a:b", &"x".repeat(65), ""] {
        let request = format!("GET /session/{bad} HTTP/1.1\r\n\r\n");
        assert_eq!(
            http(&dir, request.as_bytes()),
            "HTTP/1.1 400 Bad Request",
            "{bad}"
        );
    }
    service.stop();
}

#[test]
fn usage_records_work_with_hook_compression_off() {
    // No owner file: the hook does not compress and sends no report.
    let dir = state_dir("usageoff");
    let _cleanup = RemoveOnDrop(dir.clone());
    let service = run(&dir);
    stdout(&deslag(
        &dir,
        &["hook", "claude", "--minify-json"],
        &hook_input(),
    ));
    assert_eq!(post_usage(&dir, SESSION, 0), "HTTP/1.1 204 No Content");
    service.stop();
    let lines = requests(&dir);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["compression_reports"], 0);
    assert_eq!(lines[0]["estimated_removed_tokens"], 0);
    assert_eq!(lines[0]["usage"]["input_tokens"], 100);
}

#[test]
fn the_bridge_answers_503_when_the_datagram_cannot_be_sent() {
    let dir = state_dir("bridge503");
    let _cleanup = RemoveOnDrop(dir.clone());
    // Killed on drop: without `local.sock` no `stop` datagram can reach it.
    let _service = run(&dir);
    fs::remove_file(dir.join("local.sock")).unwrap();
    assert_eq!(
        post_usage(&dir, SESSION, 0),
        "HTTP/1.1 503 Service Unavailable"
    );
}

#[test]
fn rotation_drops_old_and_malformed_request_lines() {
    let dir = state_dir("reqrot");
    let _cleanup = RemoveOnDrop(dir.clone());
    fs::create_dir_all(&dir).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let line = |time: u64| {
        format!(
            r#"{{"v":1,"time":{time},"session":"s","turn":"t","index":0,"model":"m","usage":null,"compression_reports":0,"estimated_removed_tokens":null,"dropped":0}}"#
        )
    };
    let (old, recent) = (line(now - 2 * 24 * 3600), line(now));
    fs::write(
        dir.join("requests.jsonl"),
        format!(
            "{old}\ngarbage\n{recent}\n{}\n",
            recent.replace("\"s\"", "\"a b\"")
        ),
    )
    .unwrap();
    run_with(&dir, &["--retention-days", "1"]).stop();
    let text = fs::read_to_string(dir.join("requests.jsonl")).unwrap();
    let kept: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(
        kept,
        serde_json::from_str::<serde_json::Value>(&recent).unwrap()
    );
}
