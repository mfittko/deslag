use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

fn deslag(args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_deslag"));
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

#[test]
fn valid_start_reports_bound_address_and_redacted_upstream() {
    let mut child = deslag(&[
        "proxy",
        "--upstream",
        "https://gw.example/token-secret/",
        "--listen",
        "127.0.0.1:0",
    ])
    .spawn()
    .expect("spawn deslag");
    let mut line = String::new();
    BufReader::new(child.stderr.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    child.kill().unwrap();
    child.wait().unwrap();

    let rest = line
        .strip_prefix("deslag proxy listening on 127.0.0.1:")
        .unwrap_or_else(|| panic!("unexpected line: {line}"));
    let (port, upstream) = rest.split_once(", upstream ").unwrap();
    assert_ne!(port.parse::<u16>().unwrap(), 0);
    assert_eq!(upstream.trim_end(), "https://gw.example/[redacted]");
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
