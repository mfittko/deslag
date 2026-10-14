//! `deslag stats`: the hourly aggregates in `stats.jsonl` as a table or as
//! versioned JSON. The schema is documented in docs/observability.md.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    process::ExitCode,
};

use deslag_telemetry::{STATS_FILE, state_dir};
use serde_json::{Value, json};

use crate::local::{self, Bucket, HOUR, Key};

pub const USAGE: &str = "usage: deslag stats [--json] [--since <n>h|<n>d]";

const SCHEMA_VERSION: u64 = 1;

/// Runs `deslag stats <args>` and returns the process exit code: 2 when
/// `stats.jsonl` exists but cannot be read or when stdout cannot be written.
pub fn command(args: &[&str]) -> ExitCode {
    let Some((as_json, since_arg)) = parse_args(args) else {
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
    let text = match state_dir().map(|d| fs::read(d.join(STATS_FILE))) {
        Some(Ok(bytes)) => bytes,
        None => Vec::new(),
        Some(Err(e)) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Some(Err(e)) => {
            eprintln!("deslag: cannot read {STATS_FILE}: {e}");
            return ExitCode::from(2);
        }
    };
    let since = local::floor_hour(now.saturating_sub(since_secs));
    let stats = Stats::read(&text, since, now, running);
    let mut stdout = io::stdout().lock();
    let written = if as_json {
        writeln!(stdout, "{:#}", stats.json())
    } else {
        write!(stdout, "{}", stats.table(since_arg))
    };
    match written {
        Err(e) if e.kind() != io::ErrorKind::BrokenPipe => {
            eprintln!("deslag: cannot write stats: {e}");
            ExitCode::from(2)
        }
        _ => ExitCode::SUCCESS,
    }
}

/// `(--json, --since value)`. `None` for any other argument.
fn parse_args<'a>(args: &[&'a str]) -> Option<(bool, &'a str)> {
    let (mut as_json, mut since) = (false, "24h");
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match *arg {
            "--json" => as_json = true,
            "--since" => since = args.next()?,
            _ => return None,
        }
    }
    Some((as_json, since))
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

struct Stats {
    since: u64,
    until: u64,
    reasons: Vec<&'static str>,
    /// Report buckets only; service lines feed `dropped` and the gap check.
    buckets: BTreeMap<Key, Bucket>,
    dropped: u64,
}

impl Stats {
    /// Merges the lines with an hour in `since..=now`. Unparsable lines,
    /// invalid UTF-8 included, are skipped.
    fn read(text: &[u8], since: u64, now: u64, running: bool) -> Self {
        let mut buckets = BTreeMap::<Key, Bucket>::new();
        let mut covered = BTreeSet::new();
        let (mut dropped, mut any) = (0u64, false);
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
        }
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
        out + &format!(
            "window: last {since_arg} (since {}, until {})\n\
             emitted reduction: {} bytes; would_change proposals: {} bytes; dropped reports: {}\n\
             partial: {partial}\n",
            self.since,
            self.until,
            opt(self.reduction("changed")),
            opt(self.reduction("would_change")),
            self.dropped,
        )
    }
}
