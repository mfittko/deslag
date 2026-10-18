//! `deslag run`, `status`, `doctor`, `disable` and config validation.

use std::collections::BTreeMap;
use std::fs;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, MutexGuard};

/// A temp dir with `home/`, `state/` and `config.toml`. Removed on drop.
struct Sandbox(PathBuf);

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("deslag-workflow-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("home")).unwrap();
        Sandbox(dir)
    }

    fn state(&self) -> PathBuf {
        self.0.join("state")
    }

    fn config(&self, text: &str) -> &Self {
        fs::write(self.0.join("config.toml"), text).unwrap();
        self
    }

    /// Creates the state dir with mode 0700 and writes `owner` and `compression`.
    fn owner(&self, owner: &str, compression: &str) -> &Self {
        fs::create_dir_all(self.state()).unwrap();
        fs::set_permissions(self.state(), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(self.state().join("owner"), format!("{owner}\n")).unwrap();
        fs::write(self.state().join("compression"), format!("{compression}\n")).unwrap();
        self
    }

    /// The binary with every path under the sandbox and no Anthropic variables.
    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_deslag"));
        let home = self.0.join("home");
        cmd.args(args)
            .env("HOME", &home)
            .env("CLAUDE_CONFIG_DIR", home.join(".claude"))
            .env("DESLAG_STATE_DIR", self.state())
            .env("DESLAG_CONFIG", self.0.join("config.toml"))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("ANTHROPIC_BASE_URL")
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("ANTHROPIC_AUTH_TOKEN")
            .stdin(Stdio::null());
        cmd
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Every file under `dir` with its content.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path.clone());
                files.insert(path, Vec::new());
            } else {
                files.insert(path.clone(), fs::read(&path).unwrap());
            }
        }
    }
    files
}

const V1: &str = "config_version = 1\n";

/// Serializes the tests that bind `127.0.0.1:0`. A dead-port test drops its
/// listener and expects the port to stay free; a parallel bind could reuse it.
static PORTS: Mutex<()> = Mutex::new(());

fn ports() -> MutexGuard<'static, ()> {
    PORTS.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn invalid_configs_are_rejected_with_the_key() {
    let sandbox = Sandbox::new("invalid");
    for (body, expected) in [
        ("", "config_version is missing"),
        ("config_version = 2\n", "config_version is unsupported"),
        ("config_version = \"1\"\n", "config_version is unsupported"),
        ("bogus = 1\n", "unknown config key: bogus"),
        ("[nope]\n", "unknown config key: nope"),
        (
            "[compression]\nfoo = 1\n",
            "unknown config key: compression.foo",
        ),
        ("compression = 1\n", "compression must be of type table"),
        (
            "[compression]\nmax_parallel_jobs = \"4\"\n",
            "compression.max_parallel_jobs must be of type integer",
        ),
        (
            "[compression]\nmin_candidate_bytes = 10\nmax_candidate_bytes = 9\n",
            "compression.min_candidate_bytes must not exceed",
        ),
        (
            "[compression]\nmin_estimated_savings_ratio = 1.5\n",
            "compression.min_estimated_savings_ratio must be within 0..=1",
        ),
        (
            "[compression]\nmax_parallel_jobs = 0\n",
            "compression.max_parallel_jobs must be at least 1",
        ),
        (
            "[compression]\nprocessing_budget_ms = 0\n",
            "compression.processing_budget_ms must be at least 1",
        ),
        (
            "[compression]\nmin_candidate_bytes = -1\n",
            "compression.min_candidate_bytes must be at least 0",
        ),
        (
            "[transforms]\njson_table = true\n",
            "transforms.json_table is not implemented",
        ),
        (
            "mode = \"off\"\n",
            "mode is not configurable in this version",
        ),
        (
            "mode = \"fast\"\n",
            "mode is not configurable in this version",
        ),
        (
            "[transforms]\njson_minify = false\n",
            "transforms.json_minify is not configurable in this version",
        ),
        // A wrong-typed fixed key gets the type error, not the fixed-key error.
        (
            "[transforms]\njson_minify = \"true\"\n",
            "transforms.json_minify must be of type boolean",
        ),
        ("mode = 1\n", "mode must be of type string"),
        (
            "[telemetry]\ncontent_capture = true\n",
            "telemetry.content_capture is not configurable in this version",
        ),
        (
            "[telemetry]\notlp_enabled = true\n",
            "telemetry.otlp_enabled is not configurable in this version",
        ),
        (
            "compression = { foo = 1 }\n",
            "unknown config key: compression.foo",
        ),
        (
            "[compression.sub]\nfoo = 1\n",
            "unknown config key: compression.sub",
        ),
        (
            "[compression]\nmax_parallel_jobs = [4]\n",
            "compression.max_parallel_jobs must be of type integer",
        ),
        (
            "[compression]\nmax_parallel_jobs = 4.0\n",
            "compression.max_parallel_jobs must be of type integer",
        ),
        (
            "[compression]\nmin_estimated_savings_ratio = nan\n",
            "compression.min_estimated_savings_ratio must be within 0..=1",
        ),
        (
            "[compression]\nmin_estimated_savings_ratio = inf\n",
            "compression.min_estimated_savings_ratio must be within 0..=1",
        ),
        (
            "[compression]\nmin_estimated_savings_ratio = 0\n",
            "compression.min_estimated_savings_ratio must be of type float",
        ),
        (
            "[proxy]\nlisten = \"127.0.0.1:1\"\nlisten = \"127.0.0.1:2\"\n",
            "config line 4: duplicate key",
        ),
        (
            "[proxy]\nlisten = \"0.0.0.0:8789\"\n",
            "proxy.listen must be a loopback address",
        ),
        (
            "[proxy]\nlisten = \"nope\"\n",
            "proxy.listen is not a socket address",
        ),
        (
            "[proxy]\nupstream = \"http://api.example.com\"\n",
            "proxy.upstream: plain http",
        ),
        (
            "[proxy]\nupstream = \"ftp://api.example.com\"\n",
            "proxy.upstream: upstream URL scheme",
        ),
        (
            "[proxy]\nupstream = \"https://u:p@api.example.com\"\n",
            "proxy.upstream: upstream URL must not contain credentials",
        ),
        (
            "[proxy]\nupstream = \"http://127.0.0.1:8789\"\n",
            "proxy.upstream: points at the listener",
        ),
        (
            "[proxy]\nlisten = \"127.0.0.1:443\"\nupstream = \"https://localhost/v1\"\n",
            "proxy.upstream: points at the listener",
        ),
        // Scheme and host match case-insensitively.
        (
            "[proxy]\nlisten = \"127.0.0.1:443\"\nupstream = \"HTTPS://localhost\"\n",
            "proxy.upstream: points at the listener",
        ),
        (
            "[proxy]\nupstream = \"https://LOCALHOST:8789\"\n",
            "proxy.upstream: points at the listener",
        ),
        (
            "[proxy]\nupstream = \"http://localhost:8789\"\n",
            "proxy.upstream: points at the listener",
        ),
        // IPv6 brackets are stripped before the loopback check.
        (
            "[proxy]\nupstream = \"http://[::1]:8789\"\n",
            "proxy.upstream: points at the listener",
        ),
        (
            "[proxy]\nupstream = \"https://localhost:8789\"\n",
            "proxy.upstream: points at the listener",
        ),
        (
            "[proxy]\nupstream = \"https://127.0.0.1:8789\"\n",
            "proxy.upstream: points at the listener",
        ),
        // The unspecified address and an IPv4-mapped loopback reach the listener.
        (
            "[proxy]\nupstream = \"https://0.0.0.0:8789\"\n",
            "proxy.upstream: points at the listener",
        ),
        (
            "[proxy]\nupstream = \"https://[::]:8789\"\n",
            "proxy.upstream: points at the listener",
        ),
        (
            "[proxy]\nupstream = \"https://[::ffff:127.0.0.1]:8789\"\n",
            "proxy.upstream: points at the listener",
        ),
        // The resolver strips one trailing dot from a name.
        (
            "[proxy]\nupstream = \"https://localhost.:8789\"\n",
            "proxy.upstream: points at the listener",
        ),
        (
            "[proxy]\nupstream = \"https://LOCALHOST.:8789\"\n",
            "proxy.upstream: points at the listener",
        ),
        // getaddrinfo resolves inet_aton numeric forms, so they are refused.
        (
            "[proxy]\nupstream = \"https://127.1:8789\"\n",
            "proxy.upstream: upstream host must be a canonical IP address or a name",
        ),
        (
            "[proxy]\nupstream = \"https://2130706433:8789\"\n",
            "proxy.upstream: upstream host must be a canonical IP address or a name",
        ),
        (
            "[proxy]\nupstream = \"https://0x7f.0.0.1:8789\"\n",
            "proxy.upstream: upstream host must be a canonical IP address or a name",
        ),
        (
            "[proxy]\nupstream = \"https://0:8789\"\n",
            "proxy.upstream: upstream host must be a canonical IP address or a name",
        ),
        ("[proxy]\nupstream = 1\n", "proxy.upstream must be a string"),
        // A quoted top-level key is not the [proxy] upstream.
        (
            "\"proxy.upstream\" = \"https://api.anthropic.com\"\n",
            "unknown config key: proxy.upstream",
        ),
        // Control characters in an echoed key are escaped.
        ("\"\\u001b[2J\" = 1\n", "unknown config key: \\u{1b}[2J"),
    ] {
        let body = if body.starts_with("config_version") || body.is_empty() {
            body.to_string()
        } else {
            format!("{V1}{body}")
        };
        let output = sandbox.config(&body).run(&["doctor"]);
        let stdout = text(&output.stdout);
        assert!(!stdout.contains('\u{1b}'), "{body}: raw control character");
        assert_eq!(output.status.code(), Some(1), "{body}: {stdout}");
        assert!(
            stdout.contains(&format!("FAIL: config is invalid: {expected}")),
            "{body}: {stdout}"
        );
        // `run` refuses an invalid config before it starts a child.
        let output = sandbox.run(&["run", "--", "true"]);
        assert_eq!(output.status.code(), Some(2), "{body}");
        assert!(text(&output.stderr).contains(expected), "{body}");
    }
    // The echoed unknown key is cut to 64 characters.
    let body = format!("{V1}{} = 1\n", "k".repeat(100));
    let stdout = text(&sandbox.config(&body).run(&["doctor"]).stdout);
    let expected = format!("unknown config key: {}\n", "k".repeat(64));
    assert!(stdout.contains(&expected), "{stdout}");
}

#[test]
fn the_documented_defaults_and_a_loopback_http_upstream_are_valid() {
    let sandbox = Sandbox::new("valid");
    sandbox.owner("none", "off");
    let full = "config_version = 1\nmode = \"observe\"\n\
        [compression]\nmin_candidate_bytes = 2048\nmax_candidate_bytes = 2097152\nmax_json_depth = 64\n\
        max_table_rows = 20000\nmin_estimated_token_savings = 64\nmin_estimated_savings_ratio = 0.10\n\
        processing_budget_ms = 10\nmax_parallel_jobs = 4\n\
        [transforms]\njson_minify = true\njson_table = false\nexact_line_runs = false\nline_templates = false\n\
        [proxy]\nlisten = \"127.0.0.1:8789\"\nupstream = \"http://127.0.0.1:9000\"\ninspection_buffer_bytes = 16777216\n\
        [telemetry]\ncontent_capture = false\nlocal_reports = true\notlp_enabled = false\nqueue_capacity = 4096\n\
        export_timeout_ms = 2000\nshutdown_timeout_ms = 2000\n";
    let section = "config_version = 1\n[proxy]\nupstream = \"https://api.anthropic.com\"\n";
    let dotted = "config_version = 1\nproxy.upstream = \"https://api.anthropic.com\"\n";
    // A loopback upstream on another port than the listener is valid.
    let other_port = "config_version = 1\n[proxy]\nlisten = \"127.0.0.1:8789\"\nupstream = \"https://127.0.0.1:9999\"\n";
    let ipv6_port = "config_version = 1\n[proxy]\nlisten = \"127.0.0.1:8789\"\nupstream = \"http://[::1]:9000\"\n";
    // A trailing slash, a path, and a name with a numeric first label are valid.
    let token_path = "config_version = 1\n[proxy]\nupstream = \"https://gw.example/token/\"\n";
    let numeric_label = "config_version = 1\n[proxy]\nupstream = \"https://123.example.com\"\n";
    for body in [
        full,
        V1,
        section,
        dotted,
        other_port,
        ipv6_port,
        token_path,
        numeric_label,
    ] {
        let output = sandbox.config(body).run(&["doctor"]);
        let stdout = text(&output.stdout);
        assert_eq!(output.status.code(), Some(0), "{stdout}");
        assert!(stdout.contains("ok: config is valid"), "{stdout}");
        assert!(stdout.contains("transforms: json_minify"), "{stdout}");
    }
    // Both spellings of the upstream key set the upstream.
    for body in [section, dotted] {
        let stdout = text(&sandbox.config(body).run(&["status"]).stdout);
        assert!(
            stdout.contains("proxy upstream: https://api.anthropic.com\n"),
            "{stdout}"
        );
    }
    // A missing file means the defaults.
    fs::remove_file(sandbox.0.join("config.toml")).unwrap();
    let stdout = text(&sandbox.run(&["status"]).stdout);
    assert!(stdout.contains("(missing, defaults apply)"), "{stdout}");
    assert!(stdout.contains("proxy listen: 127.0.0.1:8789"), "{stdout}");
}

#[test]
fn run_sets_the_base_url_only_for_the_child_and_edits_no_settings() {
    let _ports = ports();
    let sandbox = Sandbox::new("run");
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = proxy.local_addr().unwrap();
    sandbox.config(&format!("{V1}[proxy]\nlisten = \"{addr}\"\n"));
    let before = snapshot(&sandbox.0);
    for args in [
        &[
            "run",
            "--",
            "sh",
            "-c",
            "printf %s \"$ANTHROPIC_BASE_URL\"; exit 3",
        ][..],
        &[
            "run",
            "sh",
            "-c",
            "printf %s \"$ANTHROPIC_BASE_URL\"; exit 3",
        ][..],
    ] {
        let output = sandbox.run(args);
        assert_eq!(output.status.code(), Some(3), "{}", text(&output.stderr));
        assert_eq!(text(&output.stdout), format!("http://{addr}"));
        assert_eq!(text(&output.stderr), "");
    }
    // A different parent value gets a redacted notice; the proxy's own value does not.
    let child = ["run", "--", "sh", "-c", "printf %s \"$ANTHROPIC_BASE_URL\""];
    for (parent, notice) in [
        (
            "https://other.example/v1?key=querysecret7Hq2".to_string(),
            "deslag: ANTHROPIC_BASE_URL was https://other.example/[redacted]; the child now goes \
             through the proxy",
        ),
        (format!("http://{addr}/"), ""),
    ] {
        let mut cmd = sandbox.command(&child);
        let output = cmd.env("ANTHROPIC_BASE_URL", &parent).output().unwrap();
        let stderr = text(&output.stderr);
        assert_eq!(text(&output.stdout), format!("http://{addr}"));
        assert!(!stderr.contains("querysecret7Hq2"), "{stderr}");
        assert_eq!(stderr.is_empty(), notice.is_empty(), "{stderr}");
        assert!(stderr.starts_with(notice), "{stderr}");
        assert!(!stderr.contains("configured upstream"), "{stderr}");
    }
    let output = sandbox.run(&["run", "--", "/nonexistent/deslag-child"]);
    assert_eq!(output.status.code(), Some(127));
    assert!(text(&output.stderr).contains("cannot run /nonexistent/deslag-child"));
    let not_executable = sandbox.0.join("config.toml");
    let output = sandbox.run(&["run", "--", not_executable.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(126));
    // No settings file, state file or config file was created or changed.
    assert_eq!(snapshot(&sandbox.0), before);
    assert!(!sandbox.0.join("home/.claude").exists());
}

#[test]
fn run_without_a_listening_proxy_fails_with_guidance() {
    let _ports = ports();
    let sandbox = Sandbox::new("run-down");
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    sandbox.config(&format!("{V1}[proxy]\nlisten = \"127.0.0.1:{port}\"\n"));
    let marker = sandbox.0.join("ran");
    let output = sandbox.run(&["run", "--", "touch", marker.to_str().unwrap()]);
    let stderr = text(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains(&format!("no deslag proxy listens on 127.0.0.1:{port}")),
        "{stderr}"
    );
    assert!(stderr.contains("deslag proxy --upstream"), "{stderr}");
    assert!(!marker.exists());
    assert_eq!(sandbox.run(&["run"]).status.code(), Some(64));
}

#[test]
fn status_states_hook_mode_proxy_mode_or_off() {
    let sandbox = Sandbox::new("status");
    sandbox.config(V1);
    for (owner, compression, mode) in [
        (
            "hook",
            "on",
            "hook mode (owner hook, /deslag:compression on)",
        ),
        ("hook", "off", "off"),
        (
            "proxy",
            "off",
            "proxy mode (owner proxy; compresses only through deslag proxy --minify-json)",
        ),
        ("none", "on", "off"),
        ("none", "off", "off"),
    ] {
        let output = sandbox.owner(owner, compression).run(&["status"]);
        let stdout = text(&output.stdout);
        assert_eq!(output.status.code(), Some(0));
        assert!(
            stdout.starts_with(&format!("deslag {}\n", env!("CARGO_PKG_VERSION"))),
            "{stdout}"
        );
        assert!(
            stdout.contains(&format!("compression: {mode}\n")),
            "{owner} {compression}: {stdout}"
        );
        assert!(stdout.contains("local service: not running"), "{stdout}");
        assert!(stdout.contains("(valid)"), "{stdout}");
    }
}

/// The toggle is on only when its first line is exactly `on`, the rule the
/// plugin hook applies with `IFS= read -r`.
#[test]
fn status_reads_the_toggle_like_the_hook() {
    let sandbox = Sandbox::new("toggle");
    sandbox.config(V1);
    for (toggle, on) in [
        ("on", true),
        ("on\n", true),
        ("on\r\n", false),
        ("on\r", false),
        ("ON", false),
        ("off", false),
    ] {
        sandbox.owner("hook", "off");
        fs::write(sandbox.state().join("compression"), toggle).unwrap();
        let stdout = text(&sandbox.run(&["status"]).stdout);
        assert_eq!(
            stdout.contains("compression: hook mode"),
            on,
            "{toggle:?}: {stdout}"
        );
    }
}

#[test]
fn config_path_falls_back_to_xdg_then_home_and_skips_empty_or_relative_values() {
    let sandbox = Sandbox::new("path");
    let xdg = sandbox.0.join("xdg");
    let home = sandbox.0.join("home");
    for (deslag_config, xdg_config_home, expected) in [
        (None, Some(xdg.as_os_str()), xdg.join("deslag/config.toml")),
        (
            Some("relative.toml"),
            Some(xdg.as_os_str()),
            xdg.join("deslag/config.toml"),
        ),
        (
            Some(""),
            Some(std::ffi::OsStr::new("")),
            home.join(".config/deslag/config.toml"),
        ),
    ] {
        let mut cmd = sandbox.command(&["status"]);
        cmd.env_remove("DESLAG_CONFIG");
        if let Some(value) = deslag_config {
            cmd.env("DESLAG_CONFIG", value);
        }
        if let Some(value) = xdg_config_home {
            cmd.env("XDG_CONFIG_HOME", value);
        }
        let stdout = text(&cmd.output().unwrap().stdout);
        assert!(
            stdout.contains(&format!("config: {} (", expected.display())),
            "{deslag_config:?} {xdg_config_home:?}: {stdout}"
        );
    }
}

const SECRETS: &[&str] = &[
    "sk-ant-api03-Zq7xR2mN9pL4vK8wT1yH6jF3bD5cA0sE",
    "sk-ant-oat01-Hk3mP9qR2vX7nL4wT8yB1jF6cD0sAeZ",
    "ghp_Q4xL9mT2vR7nK8wP1yH6jF3bD5cA0sEzZ",
    "urlpassword9F3k",
    "querysecret7Hq2",
    // The parts before `/` and `?` that a split on those characters printed.
    "user:pw7Hq2",
    "u:se4Kx",
];

#[test]
fn doctor_redacts_credentials_and_states_the_mode() {
    let sandbox = Sandbox::new("redact");
    sandbox.config(V1).owner("hook", "on");
    for (base_url, shown) in [
        (
            format!(
                "https://user:{}@gw.example/v1?key={}",
                SECRETS[3], SECRETS[4]
            ),
            "set (redacted)",
        ),
        (
            "https://user:pw7Hq2/ss@gw.example/v1".to_string(),
            "set (redacted)",
        ),
        (
            "https://u:se4Kx?cret@gw.example".to_string(),
            "set (redacted)",
        ),
        (
            format!("https://gw.example:8443/v1?key={}", SECRETS[4]),
            "https://gw.example:8443/[redacted]",
        ),
        ("https://gw.example".to_string(), "https://gw.example\n"),
    ] {
        let mut cmd = sandbox.command(&["doctor"]);
        cmd.env("ANTHROPIC_API_KEY", SECRETS[0])
            .env("ANTHROPIC_AUTH_TOKEN", SECRETS[1])
            .env("MY_SERVICE_SECRET", SECRETS[2])
            .env("ANTHROPIC_BASE_URL", &base_url);
        let output = cmd.output().unwrap();
        let (stdout, stderr) = (text(&output.stdout), text(&output.stderr));
        for secret in SECRETS {
            assert!(
                !stdout.contains(secret) && !stderr.contains(secret),
                "{secret} leaked:\n{stdout}{stderr}"
            );
        }
        assert!(
            stdout.contains("ANTHROPIC_API_KEY: set (redacted)"),
            "{stdout}"
        );
        assert!(
            stdout.contains("ANTHROPIC_AUTH_TOKEN: set (redacted)"),
            "{stdout}"
        );
        assert!(
            stdout.contains(&format!("ANTHROPIC_BASE_URL: {shown}")),
            "{stdout}"
        );
        assert!(stdout.contains("compression: hook mode"), "{stdout}");
        assert_eq!(output.status.code(), Some(0), "{stdout}");
    }

    // A config with a credential in the upstream, or in a line that does not parse.
    for body in [
        format!(
            "{V1}[proxy]\nupstream = \"https://u:{}@gw.example\"\n",
            SECRETS[3]
        ),
        format!(
            "{V1}[proxy]\nupstream = \"https://u:{}@gw.example\n",
            SECRETS[3]
        ),
        format!(
            "{V1}[proxy]\nupstream = \"https://gw.example/{}\"\n",
            SECRETS[4]
        ),
    ] {
        let output = sandbox.config(&body).owner("proxy", "off").run(&["doctor"]);
        let all = text(&output.stdout) + &text(&output.stderr);
        assert!(
            !all.contains(SECRETS[3]) && !all.contains(SECRETS[4]),
            "{all}"
        );
        assert!(all.contains("compression: proxy mode"), "{all}");
    }
}

#[test]
fn doctor_fails_on_a_bad_state_dir_owner_mismatch_or_dead_base_url() {
    let _ports = ports();
    let sandbox = Sandbox::new("checks");
    sandbox.config(V1).owner("hook", "on");
    assert_eq!(sandbox.run(&["doctor"]).status.code(), Some(0));

    fs::set_permissions(sandbox.state(), fs::Permissions::from_mode(0o755)).unwrap();
    let output = sandbox.run(&["doctor"]);
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("has mode 0755; expected 0700"), "{stdout}");

    sandbox.owner("proxy", "on");
    let output = sandbox.run(&["doctor"]);
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(
        stdout.contains("warn: compression is on but the owner is proxy")
            && stdout.contains("run /deslag:compression off between sessions"),
        "{stdout}"
    );

    sandbox.owner("none", "on");
    let output = sandbox.run(&["doctor"]);
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains("FAIL: compression is on but the owner is none"),
        "{stdout}"
    );

    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    sandbox
        .config(&format!("{V1}[proxy]\nlisten = \"127.0.0.1:{port}\"\n"))
        .owner("proxy", "off");
    let mut cmd = sandbox.command(&["doctor"]);
    cmd.env("ANTHROPIC_BASE_URL", format!("http://127.0.0.1:{port}"));
    let output = cmd.output().unwrap();
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains("but no deslag proxy listens there"),
        "{stdout}"
    );
    // A TCP connect proves a listener, not the deslag proxy.
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
    let stdout = text(&cmd.output().unwrap().stdout);
    assert!(
        stdout.contains(&format!(
            "ANTHROPIC_BASE_URL points at 127.0.0.1:{port}, and a listener accepts connections there"
        )),
        "{stdout}"
    );
    drop(listener);

    // A missing state dir is the fresh-install state.
    fs::remove_dir_all(sandbox.state()).unwrap();
    let output = sandbox.run(&["doctor"]);
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    assert!(
        stdout.contains("not created yet (compression off)"),
        "{stdout}"
    );
    // A file in place of the state dir fails.
    fs::write(sandbox.state(), "").unwrap();
    let output = sandbox.run(&["doctor"]);
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("is not a directory"), "{stdout}");
}

#[test]
fn disable_turns_compression_off_and_gives_fresh_session_guidance() {
    let sandbox = Sandbox::new("disable");
    sandbox.config(V1).owner("hook", "on");
    let output = sandbox.run(&["disable"]);
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(
        fs::read_to_string(sandbox.state().join("owner")).unwrap(),
        "none\n"
    );
    assert_eq!(
        fs::read_to_string(sandbox.state().join("compression")).unwrap(),
        "off\n"
    );
    assert!(
        stdout.contains("New tool results stay unchanged"),
        "{stdout}"
    );
    assert!(
        stdout.contains("already compressed or pinned in the current session stay as they are"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Start a new Claude Code session"),
        "{stdout}"
    );
    let output = sandbox.run(&["doctor"]);
    assert!(text(&output.stdout).contains("compression: off"));
    assert_eq!(output.status.code(), Some(0));

    // Without a state dir, disable creates it with mode 0700.
    fs::remove_dir_all(sandbox.state()).unwrap();
    assert_eq!(sandbox.run(&["disable"]).status.code(), Some(0));
    let mode = fs::metadata(sandbox.state()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    assert_eq!(
        fs::read_to_string(sandbox.state().join("owner")).unwrap(),
        "none\n"
    );

    // A file in place of the state dir fails with no success guidance.
    fs::remove_dir_all(sandbox.state()).unwrap();
    fs::write(sandbox.state(), "").unwrap();
    let output = sandbox.run(&["disable"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = text(&output.stderr);
    assert!(stderr.contains("cannot disable compression"), "{stderr}");
    let stdout = text(&output.stdout);
    assert!(
        !stdout.contains("Start a new Claude Code session"),
        "{stdout}"
    );
}
