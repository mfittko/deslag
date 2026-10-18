use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// The binary with its state directory under the system temp dir, never the user's.
fn deslag(args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_deslag"));
    let state = std::env::temp_dir().join(format!("deslag-cli-proxy-{}", std::process::id()));
    cmd.args(args)
        .env("DESLAG_STATE_DIR", &state)
        .env("DESLAG_CONFIG", state.join("config.toml"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Kills and reaps the proxy on drop, so a failing assertion never orphans it.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts the proxy with `extra` args, checks the listening line, and connects to the port.
fn assert_valid_start(extra: &[&str], host: &str) {
    let mut args = vec!["proxy", "--upstream", "https://gw.example/token-secret/"];
    args.extend_from_slice(extra);
    assert_start(deslag(&args), host);
}

fn assert_start(mut cmd: Command, host: &str) {
    let mut child = KillOnDrop(cmd.spawn().expect("spawn deslag"));
    let stderr = child.0.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stderr).read_line(&mut line);
        let _ = tx.send(line);
    });
    let line = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("no listening line within 10 s");

    let rest = line
        .strip_prefix(&format!("deslag proxy listening on {host}:"))
        .unwrap_or_else(|| panic!("unexpected line: {line}"));
    let (port, upstream) = rest.split_once(", upstream ").unwrap();
    let port = port.parse::<u16>().unwrap();
    assert_ne!(port, 0);
    assert_eq!(upstream.trim_end(), "https://gw.example/[redacted]");
    TcpStream::connect(format!("{host}:{port}")).expect("connect to proxy");
}

#[test]
fn valid_start_reports_bound_address_and_redacted_upstream() {
    assert_valid_start(&["--listen", "127.0.0.1:0"], "127.0.0.1");
}

#[test]
fn missing_flags_come_from_the_config_and_flags_win() {
    let dir = std::env::temp_dir().join(format!("deslag-cli-proxy-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("config.toml");
    std::fs::write(
        &config,
        "config_version = 1\n[proxy]\nlisten = \"127.0.0.1:0\"\nupstream = \"https://gw.example/token-secret/\"\n",
    )
    .unwrap();
    let mut cmd = deslag(&["proxy"]);
    cmd.env("DESLAG_CONFIG", &config);
    assert_start(cmd, "127.0.0.1");
    // The config listen port is taken; the flag overrides it.
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    std::fs::write(
        &config,
        format!("config_version = 1\n[proxy]\nlisten = \"127.0.0.1:{port}\"\nupstream = \"https://gw.example/token-secret/\"\n"),
    )
    .unwrap();
    let mut cmd = deslag(&["proxy", "--listen", "127.0.0.1:0"]);
    cmd.env("DESLAG_CONFIG", &config);
    assert_start(cmd, "127.0.0.1");
    // An invalid config file fails even with every flag given.
    std::fs::write(&config, "config_version = 2\n").unwrap();
    let mut cmd = deslag(&["proxy", "--upstream", "https://api.anthropic.com"]);
    cmd.env("DESLAG_CONFIG", &config);
    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("config_version"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn valid_start_with_minify_json() {
    assert_valid_start(&["--minify-json", "--listen", "127.0.0.1:0"], "127.0.0.1");
}

#[test]
fn valid_start_on_ipv6_loopback() {
    // Hosts without an IPv6 loopback (Docker's default) cannot bind [::1].
    if std::net::TcpListener::bind("[::1]:0").is_err() {
        eprintln!("skipped: no IPv6 loopback on this host");
        return;
    }
    assert_valid_start(&["--minify-json", "--listen", "[::1]:0"], "[::1]");
}

#[test]
fn rejected_arguments_exit_2_with_message() {
    let up = "https://api.anthropic.com";
    for (args, expected) in [
        (vec!["proxy"], "usage: deslag proxy"),
        (
            vec!["proxy", "--listen", "127.0.0.1:0"],
            "usage: deslag proxy",
        ),
        (vec!["proxy", "--upstream"], "--upstream needs a value"),
        (
            vec!["proxy", "--upstream", up, "--listen"],
            "--listen needs a value",
        ),
        (
            vec!["proxy", "--upstream", up, "--bogus"],
            "unknown argument: --bogus",
        ),
        (
            vec!["proxy", "--upstream", "ftp://x.example"],
            "scheme must be https or http",
        ),
        (
            vec!["proxy", "--upstream", "http://x.example"],
            "plain http upstream",
        ),
        (
            vec!["proxy", "--upstream", up, "--listen", "nope"],
            "--listen is not a socket address",
        ),
        (
            vec!["proxy", "--upstream", up, "--listen", "0.0.0.0:0"],
            "must be a loopback address",
        ),
    ] {
        let output = deslag(&args).output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {stderr}");
        assert!(stderr.starts_with("deslag: "), "{args:?}: {stderr}");
        assert!(stderr.contains(expected), "{args:?}: {stderr}");
        assert!(output.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn minify_json_without_a_state_directory_exits_2() {
    let mut cmd = deslag(&[
        "proxy",
        "--upstream",
        "https://api.anthropic.com",
        "--minify-json",
    ]);
    for var in ["DESLAG_STATE_DIR", "XDG_STATE_HOME", "HOME"] {
        cmd.env_remove(var);
    }
    let output = cmd.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("needs a state directory"), "{stderr}");
}
