//! Versioned TOML configuration.
//!
//! The file is `$DESLAG_CONFIG`, else `$XDG_CONFIG_HOME/deslag/config.toml`,
//! else `$HOME/.config/deslag/config.toml`. Empty or relative values are
//! skipped. A missing file means the defaults below. Every key is optional
//! except `config_version`, and an unknown key or section is an error.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use deslag_proxy::Upstream;
use toml::{Table, Value};

/// The defaults, and the schema: a user value must have the type of its default.
/// `proxy.upstream` has no default and is the one optional string.
const DEFAULTS: &str = r#"
config_version = 1
mode = "observe"

[compression]
min_candidate_bytes = 2048
max_candidate_bytes = 2097152
max_json_depth = 64
max_table_rows = 20000
min_estimated_token_savings = 64
min_estimated_savings_ratio = 0.10
processing_budget_ms = 10
max_parallel_jobs = 4

[transforms]
json_minify = true
json_table = false
exact_line_runs = false
line_templates = false

[proxy]
listen = "127.0.0.1:8789"
inspection_buffer_bytes = 16777216

[telemetry]
content_capture = false
local_reports = true
otlp_enabled = false
queue_capacity = 4096
export_timeout_ms = 2000
shutdown_timeout_ms = 2000
"#;

/// Keys whose only accepted value is the default: another value would claim a
/// behaviour this version does not have.
const FIXED: &[&str] = &[
    "mode",
    "transforms.json_minify",
    "telemetry.content_capture",
    "telemetry.otlp_enabled",
];
/// Transforms this version can run. Enabling any other transform is an error,
/// so the config never claims a transform that does not run.
const IMPLEMENTED_TRANSFORMS: &[&str] = &["json_minify"];

#[derive(Debug)]
pub struct Config {
    pub listen: SocketAddr,
    /// The raw upstream URL. It passed [`Upstream::parse`].
    pub upstream: Option<String>,
    pub transforms: Vec<String>,
}

/// The config file path, or `None` when no source is usable.
pub fn path() -> Option<PathBuf> {
    let var = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    var("DESLAG_CONFIG")
        .or_else(|| var("XDG_CONFIG_HOME").map(|d| d.join("deslag/config.toml")))
        .or_else(|| var("HOME").map(|d| d.join(".config/deslag/config.toml")))
}

/// Loads and validates the config at `path`. A missing file gives the defaults.
pub fn load(path: Option<&Path>) -> Result<Config, String> {
    let text = match path.map(std::fs::read_to_string) {
        None => None,
        Some(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
        Some(Err(e)) => return Err(format!("cannot read config: {e}")),
        Some(Ok(text)) => Some(text),
    };
    match text {
        None => validate(defaults()),
        Some(text) => parse(&text),
    }
}

/// Parses and validates config text.
pub fn parse(text: &str) -> Result<Config, String> {
    let user: Table = text.parse().map_err(|e: toml::de::Error| {
        // The error's Display quotes the source line, which can hold a secret.
        let newlines = |end: usize| text.bytes().take(end).filter(|&b| b == b'\n').count();
        let line = e.span().map_or(0, |s| newlines(s.start) + 1);
        format!("config line {line}: {}", e.message().trim_end())
    })?;
    match user.get("config_version") {
        None => return Err("config_version is missing; set config_version = 1".into()),
        Some(Value::Integer(1)) => {}
        Some(_) => return Err("config_version is unsupported; only 1 is supported".into()),
    }
    let mut merged = defaults();
    merge(&mut merged, user, "")?;
    validate(merged)
}

fn defaults() -> Table {
    DEFAULTS.parse().expect("built-in defaults parse")
}

/// Copies `user` over `base`. Each key must exist in `base` with the same type.
fn merge(base: &mut Table, user: Table, prefix: &str) -> Result<(), String> {
    for (key, value) in user {
        let name = format!("{prefix}{key}");
        // Match the section and key, not the joined name: a quoted top-level
        // key "proxy.upstream" is unknown.
        if prefix == "proxy." && key == "upstream" {
            if !value.is_str() {
                return Err(format!("{name} must be a string"));
            }
            base.insert(key, value);
            continue;
        }
        match (base.get_mut(&key), value) {
            (Some(Value::Table(section)), Value::Table(user)) => {
                merge(section, user, &format!("{name}."))?
            }
            // A wrong-typed fixed key falls through to the type error below.
            (Some(old), value)
                if FIXED.contains(&name.as_str()) && old.same_type(&value) && *old != value =>
            {
                return Err(format!(
                    "{name} is not configurable in this version; remove it or set the default"
                ));
            }
            (Some(old), value) if old.same_type(&value) => *old = value,
            (Some(old), _) => {
                return Err(format!("{name} must be of type {}", old.type_str()));
            }
            (None, _) => {
                // A quoted key can hold control characters: escape them.
                let name: String = name.escape_debug().take(64).collect();
                return Err(format!("unknown config key: {name}"));
            }
        }
    }
    Ok(())
}

fn validate(table: Table) -> Result<Config, String> {
    let get = |section: &str, key: &str| table.get(section).and_then(|s| s.get(key));
    let int = |section: &str, key: &str| get(section, key).and_then(Value::as_integer);
    // A `min_` threshold may be zero. Every other count, size or time must be positive.
    for (section, value) in &table {
        for (key, value) in value.as_table().into_iter().flatten() {
            if let Some(n) = value.as_integer()
                && (n < 0 || n == 0 && !key.starts_with("min_"))
            {
                let bound = if key.starts_with("min_") { "0" } else { "1" };
                return Err(format!("{section}.{key} must be at least {bound}"));
            }
        }
    }
    if int("compression", "min_candidate_bytes") > int("compression", "max_candidate_bytes") {
        return Err(
            "compression.min_candidate_bytes must not exceed compression.max_candidate_bytes"
                .into(),
        );
    }
    let ratio = get("compression", "min_estimated_savings_ratio").and_then(Value::as_float);
    if !ratio.is_some_and(|r| (0.0..=1.0).contains(&r)) {
        return Err("compression.min_estimated_savings_ratio must be within 0..=1".into());
    }
    let mut transforms = Vec::new();
    for (name, on) in table["transforms"].as_table().into_iter().flatten() {
        if on.as_bool() == Some(true) {
            if !IMPLEMENTED_TRANSFORMS.contains(&name.as_str()) {
                return Err(format!(
                    "transforms.{name} is not implemented in this version"
                ));
            }
            transforms.push(name.clone());
        }
    }
    let listen = get("proxy", "listen")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let listen = parse_listen(listen).map_err(|e| format!("proxy.listen {e}"))?;
    let upstream = get("proxy", "upstream").and_then(Value::as_str);
    if let Some(url) = upstream {
        check_upstream(listen, url).map_err(|e| format!("proxy.upstream: {e}"))?;
    }
    Ok(Config {
        listen,
        upstream: upstream.map(str::to_owned),
        transforms,
    })
}

/// A loopback socket address. PLAN §15: network exposure needs authentication first.
pub fn parse_listen(listen: &str) -> Result<SocketAddr, String> {
    let addr: SocketAddr = listen
        .parse()
        .map_err(|e| format!("is not a socket address: {e}"))?;
    if !addr.ip().is_loopback() {
        return Err(format!("must be a loopback address, got {addr}"));
    }
    Ok(addr)
}

/// Parses `url` and rejects an upstream that is the listener itself.
pub fn check_upstream(listen: SocketAddr, url: &str) -> Result<Upstream, String> {
    let upstream = Upstream::parse(url)?;
    // ponytail: any loopback host on the listen port counts as the listener;
    // a second loopback service on the same port is not supported.
    // `Upstream::parse` accepted `url`, so it parses and has an authority.
    let uri: http::Uri = url
        .parse()
        .map_err(|e| format!("invalid upstream URL: {e}"))?;
    let Some(authority) = uri.authority() else {
        return Ok(upstream);
    };
    let default_port = if uri.scheme_str() == Some("https") {
        443
    } else {
        80
    };
    let port = authority.port_u16().unwrap_or(default_port);
    let host = authority
        .host()
        .trim_start_matches('[')
        .trim_end_matches(']');
    // The resolver treats `localhost.` as `localhost`.
    let host = host.strip_suffix('.').unwrap_or(host);
    let ip = host.parse::<IpAddr>().ok();
    // getaddrinfo maps inet_aton forms such as `127.1`, `0x7f.0.0.1` and
    // `2130706433` to an address. A DNS name never ends in a numeric label.
    let last = host.rsplit('.').next().unwrap_or(host);
    let hex = last
        .strip_prefix("0x")
        .or_else(|| last.strip_prefix("0X"))
        .is_some_and(|h| h.bytes().all(|b| b.is_ascii_hexdigit()));
    let numeric = !last.is_empty() && (last.bytes().all(|b| b.is_ascii_digit()) || hex);
    if ip.is_none() && numeric {
        return Err("upstream host must be a canonical IP address or a name".to_owned());
    }
    // A connect to the unspecified address, or to an IPv4-mapped loopback,
    // reaches a loopback listener too.
    let loopback = host.eq_ignore_ascii_case("localhost")
        || ip.is_some_and(|ip| {
            let ip = ip.to_canonical();
            ip.is_loopback() || ip.is_unspecified()
        });
    if loopback && port == listen.port() {
        return Err(format!("points at the listener {listen}"));
    }
    Ok(upstream)
}
