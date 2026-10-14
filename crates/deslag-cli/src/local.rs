//! `deslag local`: the optional stats service and its control commands.
//!
//! The service receives report datagrams from adapters (see
//! `deslag_telemetry`), aggregates them in memory into hourly buckets and
//! appends each closed bucket as one line to the private `stats.jsonl`. The
//! `stop` and `flush` control messages, sent by `deslag local stop` and
//! `deslag stats`, and a fatal receive error also write the open hour. A killed
//! service loses the unwritten part of that hour, and its stale socket file
//! stays until the next `run` removes it. A failed write is logged to stderr,
//! and its buckets stay in memory for the next attempt. The log is visible only
//! under `deslag local run`: `deslag local start` discards the service's stderr.
//!
//! The service answers `ping`, `flush` and `stop` with `ok`. When the write
//! fails, `flush` and `stop` answer `flush failed: <error>` instead, and the
//! service keeps running.

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        net::UnixDatagram,
        process::CommandExt,
    },
    path::Path,
    process::{Command, ExitCode, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use deslag_core::{
    EXACT_LINE_RUNS_ID, EstimateCounter, JSON_MINIFY_ID, JSON_TABLE_ID, LINE_TEMPLATES_ID,
};
use deslag_telemetry::{MAX_MESSAGE_BYTES, SOCKET_FILE, STATS_FILE, ensure_state_dir, state_dir};
use serde_json::{Map, Value, json};

pub const USAGE: &str = "usage: deslag local start|run [--retention-days <n>]\n       \
                         deslag local stop|status";

pub(crate) const HOUR: u64 = 3600;
const DAY: u64 = 24 * HOUR;
const DEFAULT_RETENTION_DAYS: u64 = 90;
pub(crate) const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);

// Allowlists for every string a report may carry. Anything else is dropped.
const ADAPTERS: &[&str] = &["hook", "proxy"];
const TOOLS: &[&str] = &["bash", "mcp", "other"];
const TRANSFORMS: &[&str] = &[
    JSON_MINIFY_ID,
    JSON_TABLE_ID,
    EXACT_LINE_RUNS_ID,
    LINE_TEMPLATES_ID,
];
const OUTCOMES: &[&str] = &["changed", "would_change", "skipped", "failed"];
const REASONS: &[&str] = &[
    "mode_off",
    "transform_disabled",
    "below_min_bytes",
    "above_max_bytes",
    "unsupported_format",
    "depth_limit",
    "duplicate_key",
    "escaped_key",
    "insufficient_gain",
    "invalid_candidate",
];
/// Allowed (method, tokenizer) pairs: the estimate tokenizer never labels exact counts.
/// The only stored label is `estimate:deslag-estimate-v1`; `exact` is reserved
/// for a future tokenizer.
const TOKEN_LABELS: &[(&str, &str)] = &[("estimate", EstimateCounter::TOKENIZER)];
/// Held by the running service: a second `run` fails instead of stealing the socket.
const LOCK_FILE: &str = "local.lock";

/// Runs `deslag local <args>` and returns the process exit code.
pub fn command(args: &[&str]) -> ExitCode {
    let retention = match args {
        [_] => None,
        [_, "--retention-days", n] => match n.parse::<u64>() {
            Ok(n) if (1..=36_500).contains(&n) => Some(n),
            _ => return usage(),
        },
        _ => return usage(),
    };
    let result = match (args[0], retention) {
        ("run", r) => run(r.unwrap_or(DEFAULT_RETENTION_DAYS)).map(|()| String::new()),
        ("start", r) => start(r),
        ("stop", None) => match control("stop", CONTROL_TIMEOUT) {
            Err(e) if e.kind() == io::ErrorKind::InvalidData => Err(format!("stop failed: {e}")),
            stop => reachable(stop).map(|up| {
                format!(
                    "deslag local: {}",
                    if up { "stopped" } else { "not running" }
                )
            }),
        },
        ("status", None) => reachable(control("ping", CONTROL_TIMEOUT)).map(|up| {
            format!(
                "deslag local: {}\nstate dir: {}",
                if up { "running" } else { "not running" },
                state_dir().map_or_else(|| "unset".to_string(), |d| d.display().to_string())
            )
        }),
        _ => return usage(),
    };
    match result {
        Ok(message) => {
            if !message.is_empty() {
                println!("{message}");
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("deslag: {message}");
            ExitCode::from(2)
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(crate::EX_USAGE)
}

const NOT_RESPONDING: &str = "service not responding";

/// Maps a control result to whether the service answered. Only `NotFound`
/// (no state dir, no socket file) and `ConnectionRefused` (stale socket) mean
/// not running. A timeout means alive but busy. Every other error, such as a
/// failed client bind, is a failure: the service may still be running.
fn reachable(result: io::Result<()>) -> Result<bool, String> {
    match result {
        Ok(()) => Ok(true),
        Err(e) => match e.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => Ok(false),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => Err(NOT_RESPONDING.to_string()),
            _ => Err(format!("control failed: {e}")),
        },
    }
}

fn already_running(retention: Option<u64>) -> String {
    let note = if retention.is_some() {
        "; it keeps its retention (stop and start to change it)"
    } else {
        ""
    };
    format!("deslag local: already running{note}")
}

/// Spawns a detached `local run` and waits until it answers `ping`.
fn start(retention: Option<u64>) -> Result<String, String> {
    if reachable(control("ping", CONTROL_TIMEOUT))? {
        return Ok(already_running(retention));
    }
    let exe = std::env::current_exe().map_err(|e| format!("cannot find own binary: {e}"))?;
    let mut cmd = Command::new(exe);
    cmd.args(["local", "run"]);
    if let Some(days) = retention {
        cmd.args(["--retention-days", &days.to_string()]);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("cannot start service: {e}"))?;
    let deadline = Instant::now() + CONTROL_TIMEOUT;
    // Set when the child exits. A concurrent `start` may have won the lock
    // and not bound the socket yet, so polling continues until the deadline.
    let mut exited = None;
    let started = |exited: Option<_>| match exited {
        Some(_) => already_running(retention),
        None => "deslag local: started".to_string(),
    };
    while Instant::now() < deadline {
        if control("ping", Duration::from_millis(100)).is_ok() {
            return Ok(started(exited));
        }
        if exited.is_none() {
            exited = child.try_wait().ok().flatten();
        }
        thread::sleep(Duration::from_millis(50));
    }
    // A slow first answer still counts. Otherwise reap the child, so a failed
    // `start` leaves no service behind.
    if control("ping", Duration::from_millis(100)).is_ok() {
        return Ok(started(exited));
    }
    if let Some(status) = exited {
        return Err(format!(
            "service exited ({status}); run `deslag local run` to see why"
        ));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err("service did not answer within 2 s".to_string())
}

/// Sends a control `command` from a temporary bound socket and waits up to
/// `timeout` for `ok`. `InvalidData` carries an error the service replied.
/// A missing state dir is `NotFound`; [`reachable`] classifies the other errors.
pub(crate) fn control(command: &str, timeout: Duration) -> io::Result<()> {
    let dir =
        state_dir().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no state directory"))?;
    let client_path = dir.join(format!("ctl-{}.sock", std::process::id()));
    let _ = fs::remove_file(&client_path);
    let result = (|| {
        let client = UnixDatagram::bind(&client_path)?;
        client.set_read_timeout(Some(timeout))?;
        client.set_write_timeout(Some(timeout))?;
        client.send_to(command.as_bytes(), dir.join(SOCKET_FILE))?;
        let mut buf = [0; MAX_MESSAGE_BYTES];
        let n = client.recv(&mut buf)?;
        match &buf[..n] {
            b"ok" => Ok(()),
            reply => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                String::from_utf8_lossy(reply),
            )),
        }
    })();
    let _ = fs::remove_file(&client_path);
    result
}

/// The foreground service. Holds [`LOCK_FILE`] while it runs, so only one
/// service owns the socket. Returns after `stop`.
fn run(retention_days: u64) -> Result<(), String> {
    let dir = ensure_state_dir().map_err(|e| format!("cannot create state dir: {e}"))?;
    let lock =
        open_private(&dir.join(LOCK_FILE)).map_err(|e| format!("cannot open {LOCK_FILE}: {e}"))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => return Err("service already running".to_string()),
        Err(fs::TryLockError::Error(e)) => return Err(format!("cannot lock {LOCK_FILE}: {e}")),
    }
    // Client sockets left by killed `control` calls. A live client that bound
    // its socket during this startup loses its reply and reports
    // 'service not responding'.
    for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("ctl-") && name.ends_with(".sock") {
            let _ = fs::remove_file(entry.path());
        }
    }
    let socket_path = dir.join(SOCKET_FILE);
    let _ = fs::remove_file(&socket_path);
    let socket = UnixDatagram::bind(&socket_path)
        .map_err(|e| format!("cannot bind {}: {e}", socket_path.display()))?;
    let result = serve(&socket, &socket_path, &lock, &dir, retention_days);
    if result.is_err() {
        let _ = fs::remove_file(&socket_path);
    }
    result
}

fn log(e: io::Error) {
    eprintln!("deslag local: {e}");
}

/// The receive loop. IO errors in ticks and flushes are logged to stderr, and
/// their buckets stay in memory for the next attempt. The log is visible only
/// under `deslag local run`: `deslag local start` discards it.
fn serve(
    socket: &UnixDatagram,
    socket_path: &Path,
    lock: &fs::File,
    dir: &Path,
    retention_days: u64,
) -> Result<(), String> {
    let err = |e: io::Error| e.to_string();
    fs::set_permissions(socket_path, fs::Permissions::from_mode(0o600)).map_err(err)?;
    socket
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(err)?;
    let mut service = Service::new(dir.join(STATS_FILE), retention_days * DAY, now());
    refuse_symlink(&service.path).map_err(err)?;
    rotate(&service.path, now(), service.retention).unwrap_or_else(log);
    let mut buf = [0; MAX_MESSAGE_BYTES + 1];
    loop {
        let received = socket.recv_from(&mut buf);
        let now = now();
        service.tick(now).unwrap_or_else(log);
        let (n, addr) = match received {
            Ok(r) => r,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(e) => {
                service.flush(now).unwrap_or_else(log);
                return Err(e.to_string());
            }
        };
        let reply = |text: &[u8]| drop(socket.send_to_addr(text, &addr));
        let mut flushed = || {
            service.flush(now).map_err(|e| {
                let text = format!("flush failed: {e}");
                log(e);
                text
            })
        };
        match &buf[..n] {
            b"ping" => reply(b"ok"),
            b"flush" => match flushed() {
                Ok(()) => reply(b"ok"),
                Err(e) => reply(e.as_bytes()),
            },
            // On a failed flush the service logs the error, answers
            // `flush failed: <error>` and keeps running with its buckets.
            b"stop" => match flushed() {
                Ok(()) => {
                    let _ = fs::remove_file(socket_path);
                    // Unlocked before the reply, so an immediate restart wins the lock.
                    let _ = lock.unlock();
                    reply(b"ok");
                    return Ok(());
                }
                Err(e) => reply(e.as_bytes()),
            },
            message => service.record(message, now),
        }
    }
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub(crate) fn floor_hour(t: u64) -> u64 {
    t - t % HOUR
}

/// Bucket dimensions. A service line has every dimension `None`: it carries
/// service drops and marks its hour as covered. An hour can hold several
/// service lines (a restart, a drop after a `flush`), and any service line
/// marks the hour as covered. For the drop count of an hour, see [`Bucket`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Key {
    pub hour: u64,
    pub adapter: Option<&'static str>,
    pub transform: Option<&'static str>,
    pub outcome: Option<&'static str>,
    pub reason: Option<&'static str>,
    pub tool: Option<&'static str>,
}

impl Key {
    /// The service line of `hour`, also the smallest key of that hour.
    fn service(hour: u64) -> Self {
        Key {
            hour,
            adapter: None,
            transform: None,
            outcome: None,
            reason: None,
            tool: None,
        }
    }
}

/// Sums for one key. `candidate_bytes` is `None` when no report had a
/// candidate and is stored as `null`. Token sums are per `<method>:<tokenizer>`
/// label; an empty map means unknown and is stored as an empty `tokens` object.
/// On a report line, `dropped` sums the senders' `dropped` fields: reports the
/// sender failed to deliver. On a service line, it counts service drops
/// (invalid or oversize datagrams). The total drop count of an hour is the sum
/// of `dropped` over all its lines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Bucket {
    pub count: u64,
    pub original_bytes: u64,
    pub candidate_bytes: Option<u64>,
    pub emitted_bytes: u64,
    pub tokens: BTreeMap<String, Tokens>,
    pub dropped: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tokens {
    pub original: u64,
    pub candidate: Option<u64>,
    pub emitted: u64,
}

fn add_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (None, None) => None,
        _ => Some(a.unwrap_or(0).saturating_add(b.unwrap_or(0))),
    }
}

impl Bucket {
    pub(crate) fn merge(&mut self, other: &Bucket) {
        self.count = self.count.saturating_add(other.count);
        self.original_bytes = self.original_bytes.saturating_add(other.original_bytes);
        self.candidate_bytes = add_opt(self.candidate_bytes, other.candidate_bytes);
        self.emitted_bytes = self.emitted_bytes.saturating_add(other.emitted_bytes);
        for (label, t) in &other.tokens {
            let sum = self.tokens.entry(label.clone()).or_default();
            sum.original = sum.original.saturating_add(t.original);
            sum.candidate = add_opt(sum.candidate, t.candidate);
            sum.emitted = sum.emitted.saturating_add(t.emitted);
        }
        self.dropped = self.dropped.saturating_add(other.dropped);
    }
}

/// The value of `field` when it is one of `allowed`.
fn pick(v: &Value, field: &str, allowed: &[&'static str]) -> Option<&'static str> {
    let s = v.get(field)?.as_str()?;
    allowed.iter().copied().find(|a| *a == s)
}

/// `Some(None)` for `null`, `Some(Some(_))` for an allowed value, `None` otherwise.
fn dim(v: &Value, field: &str, allowed: &[&'static str]) -> Option<Option<&'static str>> {
    match v.get(field)? {
        Value::Null => Some(None),
        _ => pick(v, field, allowed).map(Some),
    }
}

fn u64_of(v: &Value, field: &str) -> Option<u64> {
    v.get(field)?.as_u64()
}

/// `Some(None)` for `null`, `None` for a missing or non-integer field.
fn opt_u64(v: &Value, field: &str) -> Option<Option<u64>> {
    match v.get(field)? {
        Value::Null => Some(None),
        n => n.as_u64().map(Some),
    }
}

/// Parses and validates one report datagram into its key and a one-report bucket.
/// `None` also for a byte size above `u32::MAX` and for a `would_change` report
/// with a null byte or token candidate; the caller counts either as a drop.
fn parse_report(message: &[u8], hour: u64) -> Option<(Key, Bucket)> {
    let v: Value = serde_json::from_slice(message).ok()?;
    let key = Key {
        hour,
        adapter: Some(pick(&v, "adapter", ADAPTERS)?),
        transform: Some(pick(&v, "transform", TRANSFORMS)?),
        outcome: Some(pick(&v, "outcome", OUTCOMES)?),
        reason: dim(&v, "reason", REASONS)?,
        tool: Some(pick(&v, "tool", TOOLS)?),
    };
    let mut tokens = BTreeMap::new();
    let t = v.get("tokens")?;
    if !t.is_null() {
        let pair = (t.get("method")?.as_str()?, t.get("tokenizer")?.as_str()?);
        let (method, tokenizer) = TOKEN_LABELS.iter().copied().find(|&p| p == pair)?;
        let sums = Tokens {
            original: u64_of(t, "original")?,
            candidate: opt_u64(t, "candidate")?,
            emitted: u64_of(t, "emitted")?,
        };
        tokens.insert(format!("{method}:{tokenizer}"), sums);
    }
    let bucket = Bucket {
        count: 1,
        original_bytes: u64_of(&v, "original_bytes")?,
        candidate_bytes: opt_u64(&v, "candidate_bytes")?,
        emitted_bytes: u64_of(&v, "emitted_bytes")?,
        tokens,
        dropped: u64_of(&v, "dropped")?,
    };
    // A size above u32::MAX is not a real tool result; it counts as a drop.
    let sizes = [bucket.original_bytes, bucket.emitted_bytes];
    if sizes
        .into_iter()
        .chain(bucket.candidate_bytes)
        .any(|n| n > u32::MAX.into())
        || !proposal_known(&key, &bucket)
    {
        return None;
    }
    Some((key, bucket))
}

/// `false` for a `would_change` bucket with a null byte or token candidate.
/// deslag-core always sets the candidate for `would_change`; a null one would
/// merge into a known sum and overstate the proposal.
fn proposal_known(key: &Key, b: &Bucket) -> bool {
    key.outcome != Some("would_change")
        || b.candidate_bytes.is_some() && b.tokens.values().all(|t| t.candidate.is_some())
}

/// One `stats.jsonl` line, schema `"v":1`.
fn line(key: &Key, b: &Bucket) -> String {
    let mut v = bucket_json(key, b);
    v["v"] = json!(1);
    v.to_string()
}

/// Parses one `stats.jsonl` line. `None` for a malformed line, another
/// schema version, a value outside the allowlists, an hour that is not a
/// multiple of `HOUR`, a partly null dimension key, or a `would_change`
/// line with a null byte or token candidate.
pub(crate) fn parse_line(text: &str) -> Option<(Key, Bucket)> {
    let v: Value = serde_json::from_str(text).ok()?;
    if u64_of(&v, "v")? != 1 {
        return None;
    }
    let key = Key {
        hour: u64_of(&v, "hour")?,
        adapter: dim(&v, "adapter", ADAPTERS)?,
        transform: dim(&v, "transform", TRANSFORMS)?,
        outcome: dim(&v, "outcome", OUTCOMES)?,
        reason: dim(&v, "reason", REASONS)?,
        tool: dim(&v, "tool", TOOLS)?,
    };
    // A report line sets adapter, transform, outcome and tool. A service line
    // sets no dimension. Any other mix, or an unaligned hour, is malformed.
    let dims = [key.adapter, key.transform, key.outcome, key.tool];
    let set = dims.iter().filter(|d| d.is_some()).count();
    if !key.hour.is_multiple_of(HOUR) || !(set == dims.len() || set == 0 && key.reason.is_none()) {
        return None;
    }
    let tokens = v
        .get("tokens")?
        .as_object()?
        .iter()
        // Unknown labels are dropped; the rest of the line stays.
        .filter(|(label, _)| {
            TOKEN_LABELS
                .iter()
                .any(|(m, t)| **label == format!("{m}:{t}"))
        })
        .map(|(label, t)| {
            let sums = Tokens {
                original: u64_of(t, "original")?,
                candidate: opt_u64(t, "candidate")?,
                emitted: u64_of(t, "emitted")?,
            };
            Some((label.clone(), sums))
        })
        .collect::<Option<_>>()?;
    let bucket = Bucket {
        count: u64_of(&v, "count")?,
        original_bytes: u64_of(&v, "original_bytes")?,
        candidate_bytes: opt_u64(&v, "candidate_bytes")?,
        emitted_bytes: u64_of(&v, "emitted_bytes")?,
        tokens,
        dropped: u64_of(&v, "dropped")?,
    };
    proposal_known(&key, &bucket).then_some((key, bucket))
}

/// The dimensions and sums of one bucket as a JSON object.
pub(crate) fn bucket_json(key: &Key, b: &Bucket) -> Value {
    let tokens: Map<String, Value> = b
        .tokens
        .iter()
        .map(|(label, t)| {
            let sums =
                json!({"original": t.original, "candidate": t.candidate, "emitted": t.emitted});
            (label.clone(), sums)
        })
        .collect();
    json!({
        "hour": key.hour,
        "adapter": key.adapter,
        "transform": key.transform,
        "outcome": key.outcome,
        "reason": key.reason,
        "tool": key.tool,
        "count": b.count,
        "original_bytes": b.original_bytes,
        "candidate_bytes": b.candidate_bytes,
        "emitted_bytes": b.emitted_bytes,
        "tokens": tokens,
        "dropped": b.dropped,
    })
}

/// In-memory aggregation. All methods take `now` so tests inject the clock.
struct Service {
    path: std::path::PathBuf,
    retention: u64,
    buckets: BTreeMap<Key, Bucket>,
    /// Start of the hour the last tick saw.
    hour: u64,
    /// Every hour before this one already has its service line on disk.
    covered: u64,
}

impl Service {
    fn new(path: std::path::PathBuf, retention: u64, now: u64) -> Self {
        Service {
            path,
            retention,
            buckets: BTreeMap::new(),
            hour: floor_hour(now),
            covered: floor_hour(now),
        }
    }

    /// Aggregates one report. An invalid report counts as a service drop, and
    /// so does one over [`MAX_MESSAGE_BYTES`]: `serve` receives into one extra
    /// byte, so a longer datagram arrives truncated to that length.
    fn record(&mut self, message: &[u8], now: u64) {
        let hour = floor_hour(now);
        let report = (message.len() <= MAX_MESSAGE_BYTES)
            .then(|| parse_report(message, hour))
            .flatten();
        match report {
            Some((key, bucket)) => self.buckets.entry(key).or_default().merge(&bucket),
            None => self.buckets.entry(Key::service(hour)).or_default().dropped += 1,
        }
    }

    /// Closes past hours when the hour changed, then applies retention.
    fn tick(&mut self, now: u64) -> io::Result<()> {
        let hour = floor_hour(now);
        if hour > self.hour {
            self.close(hour)?;
            // Advanced before rotating: a failed rotation does not repeat the close.
            self.hour = hour;
            return rotate(&self.path, now, self.retention);
        }
        self.hour = hour;
        Ok(())
    }

    /// Writes every bucket, the open hour included. Lines are additive, so the
    /// open hour may be written again later.
    fn flush(&mut self, now: u64) -> io::Result<()> {
        self.close(floor_hour(now) + HOUR)
    }

    /// Appends all buckets of hours before `before`, plus a service line for
    /// every uncovered hour inside the retention window. On a write error the
    /// buckets stay in memory for the next call, and the file is truncated back
    /// to its old length, so the retry starts on a clean line boundary.
    fn close(&mut self, before: u64) -> io::Result<()> {
        let first = self
            .covered
            .max(floor_hour(before.saturating_sub(self.retention)));
        let open = self.buckets.split_off(&Key::service(before));
        let mut closed = std::mem::replace(&mut self.buckets, open);
        for hour in (first..before).step_by(HOUR as usize) {
            closed.entry(Key::service(hour)).or_default();
        }
        let text: String = closed.iter().map(|(k, b)| line(k, b) + "\n").collect();
        let written = open_private(&self.path)
            .and_then(|f| append_or_truncate(&f, |mut f| f.write_all(text.as_bytes())));
        if let Err(e) = written {
            self.buckets.append(&mut closed);
            return Err(e);
        }
        self.covered = self.covered.max(before);
        Ok(())
    }
}

/// Runs `write` on `file` and truncates `file` back to its old length when
/// `write` fails, so a torn append leaves no partial line.
fn append_or_truncate(
    file: &fs::File,
    write: impl FnOnce(&fs::File) -> io::Result<()>,
) -> io::Result<()> {
    let len = file.metadata()?.len();
    write(file).inspect_err(|_| {
        if let Err(e) = file.set_len(len) {
            log(io::Error::new(
                e.kind(),
                format!("rollback of a torn append failed: {e}"),
            ));
        }
    })
}

// ponytail: check-then-open race; O_NOFOLLOW needs per-platform constants (libc).
fn refuse_symlink(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(io::Error::other(format!(
            "refusing symlink {}",
            path.display()
        )));
    }
    Ok(())
}

/// Opens `path` for appending, mode 0600 also when the file already existed.
/// Refuses a symlink, so the chmod cannot reach another file.
fn open_private(path: &Path) -> io::Result<fs::File> {
    refuse_symlink(path)?;
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

/// Removes lines whose hour is before the hour that contains `now - retention`
/// and merges lines with equal keys, via a 0600 temp file renamed over `path`.
/// Lines `parse_line` rejects, invalid UTF-8 included, are removed too.
// ponytail: reads the whole file; 90 days of hourly buckets stays in the low MBs.
fn rotate(path: &Path, now: u64, retention: u64) -> io::Result<()> {
    // A symlinked `path` is refused, so the rename cannot replace it.
    refuse_symlink(path)?;
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let mut text = Vec::new();
    file.read_to_end(&mut text)?;
    let cutoff = floor_hour(now.saturating_sub(retention));
    // Lossy: an invalid byte only spoils its own line, which parse_line rejects.
    let mut merged = BTreeMap::<Key, Bucket>::new();
    for (key, bucket) in String::from_utf8_lossy(&text)
        .lines()
        .filter_map(parse_line)
    {
        if key.hour >= cutoff {
            merged.entry(key).or_default().merge(&bucket);
        }
    }
    let kept: String = merged.iter().map(|(k, b)| line(k, b) + "\n").collect();
    if kept.as_bytes() == text {
        // The rewrite path gets 0600 from its temp file; an unchanged store needs the chmod,
        // done on the open handle so a path swapped after the open cannot redirect it.
        return file.set_permissions(fs::Permissions::from_mode(0o600));
    }
    // A fresh 0600 file: `create_new` never follows a planted symlink.
    let tmp = path.with_extension("jsonl.tmp");
    let _ = fs::remove_file(&tmp);
    let result = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(kept.as_bytes()))
        .and_then(|()| fs::rename(&tmp, path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_800_000_000 - 1_800_000_000 % HOUR;

    /// Removes its directory on drop.
    struct TempDir(std::path::PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn temp_file(name: &str) -> (TempDir, std::path::PathBuf) {
        let dir = std::path::PathBuf::from(format!("/tmp/dsl-cli-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(STATS_FILE);
        (TempDir(dir), path)
    }

    fn report(outcome: &str, candidate: Option<u64>, tokens: bool) -> Vec<u8> {
        let tokens = if tokens {
            json!({"method":"estimate","tokenizer":"deslag-estimate-v1","original":10,"candidate":4,"emitted":4})
        } else {
            Value::Null
        };
        json!({"adapter":"hook","tool":"bash","transform":"json-minify","outcome":outcome,
            "reason":null,"original_bytes":100,"candidate_bytes":candidate,"emitted_bytes":40,
            "tokens":tokens,"dropped":1})
        .to_string()
        .into_bytes()
    }

    fn lines(path: &Path) -> Vec<Value> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn reports_aggregate_by_hour_and_dimensions() {
        let (_dir, path) = temp_file("agg");
        let mut s = Service::new(path, 90 * DAY, T0);
        s.record(&report("changed", Some(40), true), T0 + 1);
        s.record(&report("changed", None, false), T0 + 2);
        s.record(&report("skipped", None, false), T0 + 3);
        s.record(&report("changed", None, false), T0 + HOUR);
        assert_eq!(s.buckets.len(), 3);
        let key = Key {
            hour: T0,
            adapter: Some("hook"),
            transform: Some("json-minify"),
            outcome: Some("changed"),
            reason: None,
            tool: Some("bash"),
        };
        let b = &s.buckets[&key];
        assert_eq!(
            (b.count, b.original_bytes, b.candidate_bytes, b.dropped),
            (2, 200, Some(40), 2)
        );
        assert_eq!(b.tokens["estimate:deslag-estimate-v1"].original, 10);
        let skipped = Key {
            outcome: Some("skipped"),
            ..key
        };
        assert_eq!(s.buckets[&skipped].candidate_bytes, None);
        assert!(s.buckets[&skipped].tokens.is_empty());
    }

    #[test]
    fn closed_hours_append_private_lines_with_coverage() {
        let (_dir, path) = temp_file("close");
        let mut s = Service::new(path.clone(), 90 * DAY, T0);
        s.record(&report("changed", Some(40), true), T0 + 5);
        s.tick(T0 + 10).unwrap();
        assert!(!path.exists(), "the open hour is not written");
        s.record(&report("changed", Some(40), true), T0 + HOUR);
        s.tick(T0 + 2 * HOUR + 1).unwrap();
        let got = lines(&path);
        // Two report lines and one service line per closed hour.
        assert_eq!(got.len(), 4);
        assert_eq!(got.iter().filter(|l| l["adapter"].is_null()).count(), 2);
        let first = &got[1];
        assert_eq!(first["v"], 1);
        assert_eq!(first["hour"], T0);
        assert_eq!(first["count"], 1);
        assert_eq!(first["tokens"]["estimate:deslag-estimate-v1"]["emitted"], 4);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        s.record(&report("skipped", None, false), T0 + 2 * HOUR + 5);
        s.flush(T0 + 2 * HOUR + 6).unwrap();
        s.tick(T0 + 3 * HOUR).unwrap();
        let after = lines(&path);
        assert_eq!(after.len(), 6, "flush covers the open hour once");
        let skipped = &after[5];
        assert_eq!(skipped["candidate_bytes"], Value::Null);
        assert_eq!(skipped["tokens"], json!({}));
    }

    #[test]
    fn failed_writes_keep_buckets_for_the_next_tick() {
        let (dir, path) = temp_file("retry");
        fs::remove_dir_all(&dir.0).unwrap();
        let mut s = Service::new(path.clone(), 90 * DAY, T0);
        s.record(&report("changed", None, false), T0);
        assert!(s.tick(T0 + HOUR).is_err());
        fs::create_dir_all(&dir.0).unwrap();
        s.tick(T0 + HOUR + 1).unwrap();
        assert_eq!(lines(&path).len(), 2, "one report and one service line");
    }

    #[test]
    fn a_torn_append_is_truncated_back() {
        let (_dir, path) = temp_file("torn");
        fs::write(&path, "{}\n").unwrap();
        let file = open_private(&path).unwrap();
        let torn = append_or_truncate(&file, |mut f| {
            f.write_all(b"{\"half")?;
            Err(io::Error::other("disk full"))
        });
        assert!(torn.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
    }

    #[test]
    fn failed_rotation_does_not_repeat_the_close() {
        let (_dir, path) = temp_file("rotfail");
        fs::write(&path, "garbage\n").unwrap();
        // A directory at the temp path makes the rotation fail.
        fs::create_dir(path.with_extension("jsonl.tmp")).unwrap();
        let mut s = Service::new(path.clone(), 90 * DAY, T0);
        s.record(&report("changed", None, false), T0);
        assert!(s.tick(T0 + HOUR).is_err());
        s.tick(T0 + HOUR + 1).unwrap();
        let count = fs::read_to_string(&path).unwrap().lines().count();
        assert_eq!(count, 3, "garbage, one report, one service line");
    }

    #[test]
    fn symlinks_are_refused_or_replaced() {
        let (dir, path) = temp_file("symlink");
        let victim = dir.0.join("victim");
        fs::write(&victim, "keep\n").unwrap();
        fs::set_permissions(&victim, fs::Permissions::from_mode(0o644)).unwrap();
        std::os::unix::fs::symlink(&victim, &path).unwrap();
        let mut s = Service::new(path.clone(), 90 * DAY, T0);
        assert!(s.tick(T0 + HOUR).is_err());
        let mode = fs::metadata(&victim).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
        // Rotation refuses the symlinked stats file instead of replacing it.
        assert!(rotate(&path, T0, DAY).is_err());
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&victim).unwrap(), "keep\n");

        fs::remove_file(&path).unwrap();
        fs::write(&path, "garbage\n").unwrap();
        std::os::unix::fs::symlink(&victim, path.with_extension("jsonl.tmp")).unwrap();
        rotate(&path, T0, DAY).unwrap();
        assert_eq!(fs::read_to_string(&victim).unwrap(), "keep\n");
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
    }

    #[test]
    fn rotation_drops_old_and_invalid_lines_and_merges_equal_keys() {
        let (_dir, path) = temp_file("rotate");
        let retention = 2 * DAY;
        let mut text: String = [T0 - 3 * DAY, T0 - 2 * DAY, T0 - DAY]
            .iter()
            .map(|h| line(&Key::service(*h), &Bucket::default()) + "\n")
            .collect();
        // Two equal-key T0 lines with non-zero sums: the merge must add them.
        let bucket = |n: u64| Bucket {
            count: n,
            original_bytes: 10 * n,
            candidate_bytes: Some(7 * n),
            emitted_bytes: 5 * n,
            dropped: 2 * n,
            ..Bucket::default()
        };
        text += &(line(&Key::service(T0), &bucket(1)) + "\n");
        text += &(line(&Key::service(T0), &bucket(3)) + "\n");
        text += "garbage\n{\"v\":1,\"hour\":0}\n";
        let mut v: Value =
            serde_json::from_str(&line(&Key::service(T0), &Bucket::default())).unwrap();
        v["tokens"] = json!({"/etc/passwd": {"original": 1, "candidate": null, "emitted": 1}});
        text += &(v.to_string() + "\n");
        fs::write(&path, text).unwrap();
        rotate(&path, T0 + 10, retention).unwrap();
        let got = lines(&path);
        let hours: Vec<u64> = got.iter().map(|l| l["hour"].as_u64().unwrap()).collect();
        assert_eq!(hours, [T0 - 2 * DAY, T0 - DAY, T0]);
        assert_eq!(
            got[2]["tokens"],
            json!({}),
            "unknown token labels are dropped"
        );
        let want = bucket(4);
        assert_eq!(got[2]["count"], want.count);
        assert_eq!(got[2]["original_bytes"], want.original_bytes);
        assert_eq!(got[2]["candidate_bytes"], json!(want.candidate_bytes));
        assert_eq!(got[2]["emitted_bytes"], want.emitted_bytes);
        assert_eq!(got[2]["dropped"], want.dropped);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        rotate(&path.with_extension("missing"), T0, retention).unwrap();

        // Dropping a blank line and adding the final newline keep the length.
        let valid = line(&Key::service(T0), &Bucket::default());
        fs::write(&path, format!("\n{valid}")).unwrap();
        rotate(&path, T0, retention).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), valid + "\n");

        // An unchanged store is still narrowed to 0600.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        rotate(&path, T0, retention).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    /// Each allowlist equals the `as_str()` vocabulary of its enum, and `pick`
    /// rejects any other string. The exhaustive match stops compiling when a
    /// variant is added, so the list below cannot fall behind the enum.
    macro_rules! assert_vocab {
        ($list:expr, $enum:path, [$($v:ident),+ $(,)?]) => {{
            use $enum as E;
            for v in [$(E::$v),+] {
                match v {
                    $(E::$v)|+ => {}
                }
                assert_eq!(pick(&json!({ "f": v.as_str() }), "f", $list), Some(v.as_str()));
            }
            let mut want = vec![$(E::$v.as_str()),+];
            let mut got = $list.to_vec();
            want.sort();
            got.sort();
            assert_eq!(got, want);
            assert_eq!(pick(&json!({ "f": "not_a_variant" }), "f", $list), None);
        }};
    }

    #[test]
    fn allowlists_match_enum_vocabularies() {
        assert_vocab!(ADAPTERS, deslag_telemetry::Adapter, [Hook, Proxy]);
        assert_vocab!(TOOLS, deslag_telemetry::ToolCategory, [Bash, Mcp, Other]);
        assert_vocab!(
            OUTCOMES,
            deslag_core::Outcome,
            [Changed, WouldChange, Skipped, Failed]
        );
        assert_vocab!(
            REASONS,
            deslag_core::Reason,
            [
                ModeOff,
                TransformDisabled,
                BelowMinBytes,
                AboveMaxBytes,
                UnsupportedFormat,
                DepthLimit,
                DuplicateKey,
                EscapedKey,
                InsufficientGain,
                InvalidCandidate,
            ]
        );
        // Exact counts have no allowed tokenizer yet; a new variant must be reviewed here.
        use deslag_core::TokenMethod;
        match TokenMethod::Estimate {
            TokenMethod::Estimate | TokenMethod::Exact => {}
        }
        assert_eq!(
            TOKEN_LABELS,
            [(TokenMethod::Estimate.as_str(), EstimateCounter::TOKENIZER)]
        );
    }

    #[test]
    fn rotation_drops_unaligned_hours_and_partly_null_keys() {
        let (_dir, path) = temp_file("shape");
        let full = json!({"v":1,"hour":T0,"adapter":"hook","transform":"json-minify",
            "outcome":"changed","reason":null,"tool":"bash","count":1,"original_bytes":10,
            "candidate_bytes":null,"emitted_bytes":5,"tokens":{},"dropped":0});
        let with = |field: &str, value: Value| {
            let mut v = full.clone();
            v[field] = value;
            v.to_string() + "\n"
        };
        let service = line(&Key::service(T0), &Bucket::default());
        let mut text = full.to_string() + "\n" + &service + "\n";
        // Rejected: an unaligned hour, a null adapter with other dimensions set,
        // and an adapter with a null transform, outcome or tool.
        text += &with("hour", json!(T0 + 1));
        text += &with("adapter", Value::Null);
        for field in ["transform", "outcome", "tool"] {
            text += &with(field, Value::Null);
        }
        let mut reasoned: Value = serde_json::from_str(&service).unwrap();
        reasoned["reason"] = json!("below_min_bytes");
        text += &(reasoned.to_string() + "\n");
        fs::write(&path, text).unwrap();
        rotate(&path, T0 + 10, DAY).unwrap();
        let got = lines(&path);
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[0]["adapter"].is_null(), "service line kept");
        assert_eq!(got[1]["adapter"], "hook", "full report line kept");
    }

    #[test]
    fn only_a_missing_or_refused_socket_means_not_running() {
        use io::ErrorKind::*;
        let of = |kind: io::ErrorKind| reachable(Err(io::Error::from(kind)));
        assert_eq!(reachable(Ok(())), Ok(true));
        // NotFound covers no socket file and the missing state dir from `control`.
        for kind in [NotFound, ConnectionRefused] {
            assert_eq!(of(kind), Ok(false), "{kind:?}");
        }
        for kind in [TimedOut, WouldBlock] {
            assert_eq!(of(kind), Err(NOT_RESPONDING.to_string()), "{kind:?}");
        }
        for kind in [
            PermissionDenied,
            InvalidInput,
            AddrInUse,
            NotADirectory,
            Other,
        ] {
            assert!(
                of(kind).unwrap_err().starts_with("control failed"),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn would_change_with_a_null_candidate_is_rejected() {
        let known = report("would_change", Some(40), true);
        let (key, bucket) = parse_report(&known, T0).unwrap();
        assert!(parse_line(&line(&key, &bucket)).is_some());
        assert!(parse_report(&report("would_change", None, false), T0).is_none());
        assert!(parse_report(&report("skipped", None, false), T0).is_some());
        let mut v: Value = serde_json::from_slice(&known).unwrap();
        v["tokens"]["candidate"] = Value::Null;
        assert!(parse_report(v.to_string().as_bytes(), T0).is_none());
        let mut l: Value = serde_json::from_str(&line(&key, &bucket)).unwrap();
        l["tokens"]["estimate:deslag-estimate-v1"]["candidate"] = Value::Null;
        assert!(parse_line(&l.to_string()).is_none());
        l = serde_json::from_str(&line(&key, &bucket)).unwrap();
        l["candidate_bytes"] = Value::Null;
        assert!(parse_line(&l.to_string()).is_none());
    }
}
