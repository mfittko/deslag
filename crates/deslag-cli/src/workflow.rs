//! `deslag run`, `status`, `doctor` and `disable`.
//!
//! Output never contains a credential: an environment value whose name holds
//! KEY, TOKEN or SECRET prints as `set (redacted)`. A URL with userinfo, or a
//! value that does not parse as an absolute URL, prints as `set (redacted)`.
//! Any other printed URL keeps `scheme://host[:port]` and prints a path or
//! query as `/[redacted]`.

use std::net::{SocketAddr, TcpStream};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::time::Duration;

use deslag_proxy::session::{self, Owner};
use deslag_telemetry as report;

use crate::config::{self, Config};
use crate::local;

const RUN_USAGE: &str = "usage: deslag run [--] <command> [args...]";

/// Prints `deslag: <message>` on stderr and returns exit code 2.
fn fail(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("deslag: {message}");
    ExitCode::from(2)
}

/// Launches a child with `ANTHROPIC_BASE_URL` pointing at the configured
/// proxy. Only the child's environment changes; no settings file is written.
pub fn run(args: &[&str]) -> ExitCode {
    let args = args.strip_prefix(&["--"]).unwrap_or(args);
    let Some((program, rest)) = args.split_first() else {
        eprintln!("{RUN_USAGE}");
        return ExitCode::from(crate::EX_USAGE);
    };
    let config = match config::load(config::path().as_deref()) {
        Ok(config) => config,
        Err(e) => return fail(format!("invalid config: {e}; run `deslag doctor`")),
    };
    let listen = config.listen;
    if !proxy_listens(listen) {
        return fail(format!(
            "no deslag proxy listens on {listen}. Start one first, for example \
             `deslag proxy --upstream https://api.anthropic.com --minify-json`, \
             and make the proxy the owner"
        ));
    }
    let proxy = format!("http://{listen}");
    if let Ok(old) = std::env::var("ANTHROPIC_BASE_URL")
        && old.trim_end_matches('/') != proxy
    {
        eprintln!(
            "deslag: ANTHROPIC_BASE_URL was {}; the child now goes through the proxy on {listen}",
            redact_url(&old)
        );
    }
    // exec replaces this process, so the child gets the terminal's signals
    // directly and its exit status is the status of `deslag run`.
    let error = Command::new(program)
        .args(rest)
        .env("ANTHROPIC_BASE_URL", proxy)
        .exec();
    eprintln!("deslag: cannot run {program}: {error}");
    // The shell's codes: 127 for a missing command, 126 for one that cannot run.
    ExitCode::from(if error.kind() == std::io::ErrorKind::NotFound {
        127
    } else {
        126
    })
}

fn proxy_listens(addr: SocketAddr) -> bool {
    TcpStream::connect_timeout(&addr, Duration::from_secs(1)).is_ok()
}

/// Facts that `status` prints and `doctor` checks.
struct Facts {
    config_path: Option<PathBuf>,
    config: Result<Config, String>,
    state_dir: Option<PathBuf>,
    owner: Owner,
    toggle_on: bool,
}

impl Facts {
    fn read() -> Self {
        let config_path = config::path();
        let config = config::load(config_path.as_deref());
        let state_dir = session::state_dir();
        let (owner, toggle_on) = state_dir.as_deref().map_or((Owner::None, false), |dir| {
            let toggle = std::fs::read_to_string(dir.join("compression")).unwrap_or_default();
            (
                session::read_owner(dir),
                // Same rule as deslag-hook.sh: the first line is exactly `on`, so a
                // trailing `\r` reads as off.
                toggle.split('\n').next() == Some("on"),
            )
        });
        Facts {
            config_path,
            config,
            state_dir,
            owner,
            toggle_on,
        }
    }

    /// The owner decides which adapter compresses. The plugin hook passes
    /// `--minify-json` only while the toggle is on.
    fn mode(&self) -> &'static str {
        match (self.owner, self.toggle_on) {
            (Owner::Proxy, _) => {
                "proxy mode (owner proxy; compresses only through deslag proxy --minify-json)"
            }
            (Owner::Hook, true) => "hook mode (owner hook, /deslag:compression on)",
            _ => "off",
        }
    }

    fn print(&self) {
        let display = |p: &Option<PathBuf>| {
            p.as_ref()
                .map_or("unset".into(), |p| p.display().to_string())
        };
        println!("deslag {}", env!("CARGO_PKG_VERSION"));
        let validity = match &self.config {
            Ok(_) if self.config_path.as_ref().is_some_and(|p| p.exists()) => "valid".to_string(),
            Ok(_) => "missing, defaults apply".to_string(),
            Err(e) => format!("invalid: {e}"),
        };
        println!("config: {} ({validity})", display(&self.config_path));
        println!("compression: {}", self.mode());
        println!("owner: {}", owner_name(self.owner));
        if let Ok(config) = &self.config {
            let transforms = config.transforms.join(", ");
            println!(
                "transforms: {}",
                if transforms.is_empty() {
                    "none"
                } else {
                    &transforms
                }
            );
            println!("proxy listen: {}", config.listen);
            // Display keeps `scheme://host[:port]` and prints a base path as `/[redacted]`.
            let upstream = config
                .upstream
                .as_deref()
                .and_then(|u| config::check_upstream(config.listen, u).ok());
            println!(
                "proxy upstream: {}",
                upstream.map_or("unset".into(), |u| u.to_string())
            );
        }
        println!("state dir: {}", display(&self.state_dir));
        let local = local::control("ping", local::CONTROL_TIMEOUT).is_ok();
        println!(
            "local service: {}",
            if local { "running" } else { "not running" }
        );
    }
}

fn owner_name(owner: Owner) -> &'static str {
    match owner {
        Owner::Hook => "hook",
        Owner::Proxy => "proxy",
        Owner::None => "none",
    }
}

pub fn status() -> ExitCode {
    Facts::read().print();
    ExitCode::SUCCESS
}

/// Prints the status facts and the checks. Exits 1 when a check fails.
pub fn doctor() -> ExitCode {
    let facts = Facts::read();
    facts.print();
    for name in [
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
    ] {
        println!("{name}: {}", shown(name));
    }
    let mut failed = false;
    let mut check = |result: Result<String, String>| {
        match &result {
            Ok(message) => println!("ok: {message}"),
            Err(message) => println!("FAIL: {message}"),
        }
        failed |= result.is_err();
    };
    check(check_state_dir(facts.state_dir.as_ref()));
    check(match &facts.config {
        Ok(_) => Ok("config is valid".into()),
        Err(e) => Err(format!("config is invalid: {e}")),
    });
    // The proxy ignores the toggle, so `on` with owner proxy only warns. With
    // owner none nothing compresses although the toggle asks for it: a failure.
    if facts.toggle_on && facts.owner == Owner::Proxy {
        println!(
            "warn: compression is on but the owner is proxy, so the hook does not compress; \
             run /deslag:compression off between sessions"
        );
    } else if facts.toggle_on && facts.owner == Owner::None {
        check(Err(
            "compression is on but the owner is none, so the hook does not compress; \
             run /deslag:compression on, or `deslag disable`"
                .into(),
        ));
    } else {
        check(Ok(format!(
            "compression {} matches owner {}",
            facts.mode(),
            owner_name(facts.owner)
        )));
    }
    if let Ok(config) = &facts.config {
        check(check_base_url(config.listen));
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn check_state_dir(dir: Option<&PathBuf>) -> Result<String, String> {
    let dir = dir.ok_or("no state directory: set DESLAG_STATE_DIR, XDG_STATE_HOME or HOME")?;
    let shown = dir.display();
    let meta = match std::fs::metadata(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(format!(
                "state dir {shown} not created yet (compression off)"
            ));
        }
        result => result.map_err(|e| format!("state dir {shown}: {e}"))?,
    };
    if !meta.is_dir() {
        return Err(format!("state dir {shown} is not a directory"));
    }
    if Some(meta.uid()) != report::current_uid().ok() {
        return Err(format!(
            "state dir {shown} is not owned by the current user"
        ));
    }
    let mode = meta.permissions().mode() & 0o777;
    if mode != 0o700 {
        return Err(format!(
            "state dir {shown} has mode {mode:04o}; expected 0700"
        ));
    }
    Ok(format!(
        "state dir {shown} is owned by the current user with mode 0700"
    ))
}

fn check_base_url(listen: SocketAddr) -> Result<String, String> {
    let proxy = format!("http://{listen}");
    match std::env::var("ANTHROPIC_BASE_URL") {
        Err(_) => Ok(format!(
            "ANTHROPIC_BASE_URL is unset here; `deslag run` sets it to {proxy} for its child only"
        )),
        Ok(url) if url.trim_end_matches('/') == proxy => match proxy_listens(listen) {
            true => Ok(format!(
                "ANTHROPIC_BASE_URL points at {listen}, and a listener accepts connections there"
            )),
            false => Err(format!(
                "ANTHROPIC_BASE_URL points at {listen}, but no deslag proxy listens there"
            )),
        },
        Ok(url) => Ok(format!(
            "ANTHROPIC_BASE_URL is {}, which differs from the configured listener {proxy}",
            redact_url(&url)
        )),
    }
}

/// An environment value safe to print.
fn shown(name: &str) -> String {
    match std::env::var(name) {
        Err(_) => "unset".into(),
        Ok(_) if ["KEY", "TOKEN", "SECRET"].iter().any(|s| name.contains(s)) => {
            "set (redacted)".into()
        }
        Ok(value) => redact_url(&value),
    }
}

/// Keeps `scheme://host[:port]`. A path or query, which can embed a
/// credential, prints as `/[redacted]`. A value with `@` (userinfo) or one
/// that does not parse as an absolute URI prints as `set (redacted)`.
fn redact_url(url: &str) -> String {
    let uri = match url.parse::<http::Uri>() {
        Ok(uri) if !url.contains('@') => uri,
        _ => return "set (redacted)".into(),
    };
    let (Some(scheme), Some(authority)) = (uri.scheme_str(), uri.authority()) else {
        return "set (redacted)".into();
    };
    let path = uri.path_and_query().map_or("/", |p| p.as_str());
    let path = if path == "/" { "" } else { "/[redacted]" };
    format!("{scheme}://{authority}{path}")
}

/// Sets the owner to `none` and the hook compression toggle to `off`.
pub fn disable() -> ExitCode {
    let written = report::ensure_state_dir().and_then(|dir| {
        session::write_owner(&dir, Owner::None)?;
        session::commit(&dir.join("compression"), b"off\n")?;
        Ok(dir)
    });
    match written {
        Ok(dir) => {
            println!(
                "deslag: compression disabled (owner none, compression off in {}).\n\
                 New tool results stay unchanged from now on, in hook mode and in proxy mode.\n\
                 Results already compressed or pinned in the current session stay as they are.\n\
                 A session launched with `deslag run` still routes through the proxy, which now forwards new results unchanged.\n\
                 Start a new Claude Code session for an unchanged history.",
                dir.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => fail(format!("cannot disable compression: {e}")),
    }
}
