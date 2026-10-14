//! `deslag stats`: the hourly aggregates in `stats.jsonl` as a table or as
//! versioned JSON. The schema is documented in docs/observability.md.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    process::ExitCode,
};

use deslag_telemetry::{REQUESTS_FILE, STATS_FILE, state_dir};
use serde_json::{Value, json};

use crate::local::{self, Bucket, HOUR, Key, Request, USAGE_FIELDS};

pub const USAGE: &str = "usage: deslag stats [--json [--session <id>]] [--since <n>h|<n>d]";

const SCHEMA_VERSION: u64 = 1;

/// Runs `deslag stats <args>` and returns the process exit code: 2 when
/// `stats.jsonl` or `requests.jsonl` exists but cannot be read, or when stdout
/// cannot be written.
pub fn command(args: &[&str]) -> ExitCode {
    let Some((as_json, since_arg, session)) = parse_args(args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(crate::EX_USAGE);
    };
    let Some(since_secs) = parse_duration(since_arg) else {
        eprintln!("{USAGE}");
        return ExitCode::from(crate::EX_USAGE);
    };
    // A running service writes its open hour first, so the current hour is complete.
    let running = local::control("flush", local::CONTROL_TIMEOUT).is_ok();
    // Undocumented fixed clock for tests.
    let now = std::env::var("DESLAG_NOW")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(local::now);
    let read = |name: &str| match state_dir().map(|d| fs::read(d.join(name))) {
        Some(Ok(bytes)) => Ok(bytes),
        None => Ok(Vec::new()),
        Some(Err(e)) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Some(Err(e)) => {
            eprintln!("deslag: cannot read {name}: {e}");
            Err(ExitCode::from(2))
        }
    };
    let (text, requests) = match (read(STATS_FILE), read(REQUESTS_FILE)) {
        (Ok(text), Ok(requests)) => (text, requests),
        (Err(code), _) | (_, Err(code)) => return code,
    };
    let since = local::floor_hour(now.saturating_sub(since_secs));
    let stats = Stats::read(&text, &requests, since, now, running, session);
    let mut stdout = io::stdout().lock();
    let written = match session {
        Some(session) => writeln!(stdout, "{:#}", stats.session_json(session)),
        None if as_json => writeln!(stdout, "{:#}", stats.json()),
        None => write!(stdout, "{}", stats.table(since_arg)),
    };
    match written {
        Err(e) if e.kind() != io::ErrorKind::BrokenPipe => {
            eprintln!("deslag: cannot write stats: {e}");
            ExitCode::from(2)
        }
        _ => ExitCode::SUCCESS,
    }
}

/// `(--json, --since value, --session value)`. `None` for any other argument,
/// for `--session` without `--json` and for an invalid session ID.
fn parse_args<'a>(args: &[&'a str]) -> Option<(bool, &'a str, Option<&'a str>)> {
    let (mut as_json, mut since, mut session) = (false, "24h", None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match *arg {
            "--json" => as_json = true,
            "--since" => since = args.next()?,
            "--session" => session = Some(*args.next()?),
            _ => return None,
        }
    }
    if session.is_some_and(|s| !as_json || !deslag_telemetry::valid_id(s)) {
        return None;
    }
    Some((as_json, since, session))
}

/// `<n>h` or `<n>d` with `n >= 1`, in seconds.
fn parse_duration(s: &str) -> Option<u64> {
    let unit = match s.chars().last()? {
        'h' => HOUR,
        'd' => 24 * HOUR,
        _ => return None,
    };
    let digits = &s[..s.len() - 1];
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u64 = digits.parse().ok().filter(|n| *n >= 1)?;
    n.checked_mul(unit)
}

/// Bytes or tokens a row saved: emitted savings, or the proposal of a
/// `would_change` row. `None` when the candidate size is unknown.
fn saving(
    outcome: Option<&str>,
    original: u64,
    candidate: Option<u64>,
    emitted: u64,
) -> Option<u64> {
    match outcome {
        Some("would_change") => candidate.map(|c| original.saturating_sub(c)),
        _ => Some(original.saturating_sub(emitted)),
    }
}

/// Usage totals over `requests.jsonl` lines. `estimated_removed_tokens` is
/// `None` when one line has it unknown.
struct Usage {
    requests: u64,
    unknown_usage: u64,
    tokens: [u64; 4],
    estimated_removed_tokens: Option<u64>,
}

impl Default for Usage {
    fn default() -> Self {
        Usage {
            requests: 0,
            unknown_usage: 0,
            tokens: [0; 4],
            estimated_removed_tokens: Some(0),
        }
    }
}

impl Usage {
    fn add(&mut self, r: &Request) {
        self.requests = self.requests.saturating_add(1);
        match r.usage {
            Some(u) => {
                for (sum, n) in self.tokens.iter_mut().zip(u) {
                    *sum = sum.saturating_add(n);
                }
            }
            None => self.unknown_usage = self.unknown_usage.saturating_add(1),
        }
        self.estimated_removed_tokens = self
            .estimated_removed_tokens
            .zip(r.estimated_removed_tokens)
            .map(|(a, b)| a.saturating_add(b));
    }

    fn json(&self) -> Value {
        let mut v = json!({
            "requests": self.requests,
            "unknown_usage": self.unknown_usage,
            "estimated_removed_tokens": self.estimated_removed_tokens,
        });
        for (field, n) in USAGE_FIELDS.iter().zip(self.tokens) {
            v[field] = n.into();
        }
        v
    }
}

struct Stats {
    since: u64,
    until: u64,
    reasons: Vec<&'static str>,
    /// Report buckets only; service lines feed `dropped` and the gap check.
    buckets: BTreeMap<Key, Bucket>,
    dropped: u64,
    /// Totals over the window's requests, of one session when one is given.
    usage: Usage,
    /// The window's requests of the given session; empty without one.
    requests: Vec<Request>,
}

impl Stats {
    /// Merges the lines with an hour or time in `since..=now`. Unparsable
    /// lines, invalid UTF-8 included, are skipped. With `session`, usage
    /// totals and the request list cover that session only.
    fn read(
        text: &[u8],
        requests: &[u8],
        since: u64,
        now: u64,
        running: bool,
        session: Option<&str>,
    ) -> Self {
        let mut buckets = BTreeMap::<Key, Bucket>::new();
        let mut covered = BTreeSet::new();
        let (mut dropped, mut any) = (0u64, false);
        let mut usage = Usage::default();
        let mut listed = Vec::new();
        for r in local::parse_requests(requests) {
            if !(since..=now).contains(&r.time) {
                continue;
            }
            dropped = dropped.saturating_add(r.dropped);
            match session {
                None => usage.add(&r),
                Some(s) if s == r.session => {
                    usage.add(&r);
                    listed.push(r);
                }
                Some(_) => {}
            }
        }
        for (key, bucket) in local::parse_lines(text) {
            if !(since..=now).contains(&key.hour) {
                continue;
            }
            any = true;
            dropped = dropped.saturating_add(bucket.dropped);
            if key.adapter.is_none() {
                covered.insert(key.hour);
            } else {
                buckets.entry(key).or_default().merge(&bucket);
            }
        }
        // Only closed hours can have a gap; the current hour depends on the service.
        let closed = (since..local::floor_hour(now)).step_by(HOUR as usize);
        let checks = [
            ("service_not_running", !running),
            ("drops", dropped > 0),
            ("gaps", closed.clone().any(|h| !covered.contains(&h))),
            ("no_data", !any),
        ];
        Stats {
            since,
            until: now,
            reasons: checks.iter().filter(|c| c.1).map(|c| c.0).collect(),
            buckets,
            dropped,
            usage,
            requests: listed,
        }
    }

    /// The per-request view of one session.
    fn session_json(&self, session: &str) -> Value {
        let requests: Vec<Value> = self
            .requests
            .iter()
            .map(|r| {
                let mut v = r.json();
                let fields = v.as_object_mut().expect("request json is an object");
                fields.remove("v");
                fields.remove("session");
                v
            })
            .collect();
        json!({
            "schema_version": SCHEMA_VERSION,
            "session": session,
            "window": {"since": self.since, "until": self.until},
            "partial": {"value": !self.reasons.is_empty(), "reasons": self.reasons},
            "totals": self.usage.json(),
            "requests": requests,
        })
    }

    /// Saturating sum of [`saving`] over the buckets with `outcome`; `None`
    /// if one is unknown.
    fn reduction(&self, outcome: &str) -> Option<u64> {
        self.buckets
            .iter()
            .filter(|(k, _)| k.outcome == Some(outcome))
            .map(|(k, b)| {
                saving(
                    k.outcome,
                    b.original_bytes,
                    b.candidate_bytes,
                    b.emitted_bytes,
                )
            })
            .try_fold(0u64, |sum, s| Some(sum.saturating_add(s?)))
    }

    fn json(&self) -> Value {
        json!({
            "schema_version": SCHEMA_VERSION,
            "window": {"since": self.since, "until": self.until},
            "partial": {"value": !self.reasons.is_empty(), "reasons": self.reasons},
            "totals": {
                "reports": self.buckets.values().fold(0u64, |sum, b| sum.saturating_add(b.count)),
                "emitted_reduction_bytes": self.reduction("changed"),
                "would_change_reduction_bytes": self.reduction("would_change"),
                "dropped": self.dropped,
                "usage": self.usage.json(),
            },
            "buckets": self
                .buckets
                .iter()
                .map(|(k, b)| local::bucket_json(k, b))
                .collect::<Vec<_>>(),
        })
    }

    /// One row per dimension set, summed across the window's hours.
    fn table(&self, since_arg: &str) -> String {
        let mut rows = BTreeMap::<Key, Bucket>::new();
        for (key, bucket) in &self.buckets {
            rows.entry(Key { hour: 0, ..*key })
                .or_default()
                .merge(bucket);
        }
        let opt = |v: Option<u64>| v.map_or_else(|| "-".to_string(), |n| n.to_string());
        let row = |cells: [&str; 10]| {
            let [a, t, o, r, tool, c, orig, e, s, ts] = cells;
            format!(
                "{a:<7} {t:<15} {o:<12} {r:<18} {tool:<5} {c:>7} {orig:>10} {e:>10} {s:>10} {ts:>12}\n"
            )
        };
        let mut out = row([
            "adapter",
            "transform",
            "outcome",
            "reason",
            "tool",
            "count",
            "original",
            "emitted",
            "saved",
            "tokens_saved",
        ]);
        for (k, b) in &rows {
            // ponytail: shows the first token label only; one counter is in use today.
            let tokens = b.tokens.values().next();
            let token_saving =
                tokens.and_then(|t| saving(k.outcome, t.original, t.candidate, t.emitted));
            out += &row([
                k.adapter.unwrap_or("-"),
                k.transform.unwrap_or("-"),
                k.outcome.unwrap_or("-"),
                k.reason.unwrap_or("-"),
                k.tool.unwrap_or("-"),
                &b.count.to_string(),
                &b.original_bytes.to_string(),
                &b.emitted_bytes.to_string(),
                &opt(saving(
                    k.outcome,
                    b.original_bytes,
                    b.candidate_bytes,
                    b.emitted_bytes,
                )),
                &opt(token_saving),
            ]);
        }
        let partial = if self.reasons.is_empty() {
            "no".to_string()
        } else {
            self.reasons.join(", ")
        };
        let u = &self.usage;
        out + &format!(
            "window: last {since_arg} (since {}, until {})\n\
             emitted reduction: {} bytes; would_change proposals: {} bytes; dropped reports: {}\n\
             usage: {} requests ({} unknown); input {}, output {}, cache read {}, \
             cache creation {} tokens; estimated removed {} tokens\n\
             partial: {partial}\n",
            self.since,
            self.until,
            opt(self.reduction("changed")),
            opt(self.reduction("would_change")),
            self.dropped,
            u.requests,
            u.unknown_usage,
            u.tokens[0],
            u.tokens[1],
            u.tokens[2],
            u.tokens[3],
            opt(u.estimated_removed_tokens),
        )
    }
}
