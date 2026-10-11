use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn deslag(args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_deslag"));
    cmd.args(args)
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
    let mut child = KillOnDrop(deslag(&args).spawn().expect("spawn deslag"));
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
            "invalid --listen nope",
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
