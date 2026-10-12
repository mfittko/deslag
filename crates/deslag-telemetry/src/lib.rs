//! Report sender for the optional local stats service.
//!
//! Adapters send one small datagram per compression result to a Unix socket in
//! a private state directory. Sending never blocks and never fails the caller:
//! a missing service, a full buffer or an oversize message drops the report.
//! A message carries bounded labels and sizes only: no payload content, session
//! IDs, paths, raw tool names or request IDs.

use std::{
    fs, io,
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, PermissionsExt},
        net::UnixDatagram,
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use deslag_core::{
    CompressionContext, CompressionReport, CompressionResult, EstimateCounter, Outcome, Reason,
    TokenCounter, compress,
};
use serde_json::json;

/// Hard cap on one datagram, report or control message.
pub const MAX_MESSAGE_BYTES: usize = 512;
/// Service socket file name inside [`state_dir`].
pub const SOCKET_FILE: &str = "local.sock";
/// Aggregate store file name inside [`state_dir`].
pub const STATS_FILE: &str = "stats.jsonl";

/// `$DESLAG_STATE_DIR`, else `$XDG_STATE_HOME/deslag`, else
/// `$HOME/.local/state/deslag`. Empty or relative values are ignored, as the
/// XDG spec requires. `None` when no source is usable.
pub fn state_dir() -> Option<PathBuf> {
    let var = |name| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    var("DESLAG_STATE_DIR")
        .or_else(|| var("XDG_STATE_HOME").map(|d| d.join("deslag")))
        .or_else(|| var("HOME").map(|d| d.join(".local/state/deslag")))
}

/// Creates [`state_dir`] with mode 0700. A symlink at that path is followed.
/// Fails unless the target is a directory owned by the current user; that
/// directory gets mode 0700 again.
// ponytail: check-then-use. A symlink in a shared parent can be retargeted
// between the owner check and later use; opening the dir once and using
// fd-relative calls (openat/fchmod) would close that window.
pub fn ensure_state_dir() -> io::Result<PathBuf> {
    let dir = state_dir().ok_or_else(|| {
        io::Error::other("no state directory: set DESLAG_STATE_DIR, XDG_STATE_HOME or HOME")
    })?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    let meta = fs::metadata(&dir)?;
    if !meta.is_dir() {
        return Err(io::Error::other(format!(
            "state dir {} is not a directory",
            dir.display()
        )));
    }
    if meta.uid() != current_uid()? {
        return Err(io::Error::other(format!(
            "state dir {} is not owned by the current user",
            dir.display()
        )));
    }
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

/// The effective uid. On Linux and macOS, fstat on an anonymous pipe reports
/// the effective uid of the process that created the pipe. No file is written
/// anywhere, so a 0500 state dir or an unusable TMPDIR still passes.
// ponytail: std has no getuid; the libc crate would replace this probe.
fn current_uid() -> io::Result<u32> {
    let (reader, _writer) = io::pipe()?;
    let file = fs::File::from(std::os::fd::OwnedFd::from(reader));
    Ok(file.metadata()?.uid())
}

/// The service socket path when the socket file exists. One `stat`, so adapters
/// pay nothing else while no service has been started.
pub fn live_socket() -> Option<PathBuf> {
    state_dir()
        .map(|d| d.join(SOCKET_FILE))
        .filter(|p| p.exists())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adapter {
    Hook,
    Proxy,
}

impl Adapter {
    pub fn as_str(self) -> &'static str {
        match self {
            Adapter::Hook => "hook",
            Adapter::Proxy => "proxy",
        }
    }
}

/// Bounded tool dimension. Raw tool names never leave the adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolCategory {
    Bash,
    Mcp,
    Other,
}

impl ToolCategory {
    pub fn from_tool_name(name: &str) -> Self {
        match name {
            "Bash" => ToolCategory::Bash,
            n if n.starts_with("mcp__") => ToolCategory::Mcp,
            _ => ToolCategory::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ToolCategory::Bash => "bash",
            ToolCategory::Mcp => "mcp",
            ToolCategory::Other => "other",
        }
    }
}

/// One compact JSON object. `tokens` is `null` without counts; its label comes
/// from `original_tokens`, since one counter produces all three counts.
/// `dropped` is the number of earlier reports this sender failed to deliver.
pub fn encode(
    report: &CompressionReport,
    adapter: Adapter,
    tool: ToolCategory,
    dropped: u64,
) -> String {
    let tokens = report.original_tokens.map(|t| {
        json!({
            "method": t.method.as_str(),
            "tokenizer": t.tokenizer,
            "original": t.count,
            "candidate": report.candidate_tokens.map(|c| c.count),
            "emitted": report.emitted_tokens.map(|e| e.count),
        })
    });
    json!({
        "adapter": adapter.as_str(),
        "tool": tool.as_str(),
        "transform": report.transform_id,
        "outcome": report.outcome.as_str(),
        "reason": report.reason.map(Reason::as_str),
        "original_bytes": report.original_bytes,
        "candidate_bytes": report.candidate_bytes,
        "emitted_bytes": report.emitted_bytes,
        "tokens": tokens,
        "dropped": dropped,
    })
    .to_string()
}

/// Sends one datagram to `socket` without blocking. Returns false on any error:
/// no socket, no listener, a full receive buffer, or `message` above
/// [`MAX_MESSAGE_BYTES`].
pub fn send(socket: &Path, message: &[u8]) -> bool {
    if message.len() > MAX_MESSAGE_BYTES {
        return false;
    }
    let Ok(sock) = UnixDatagram::unbound() else {
        return false;
    };
    sock.set_nonblocking(true).is_ok()
        && sock
            .send_to(message, socket)
            .is_ok_and(|n| n == message.len())
}

/// Reports this process failed to deliver and has not yet carried in a
/// later report's `dropped` field.
static DROPPED: AtomicU64 = AtomicU64::new(0);

/// Runs [`compress`](deslag_core::compress) and, with `socket`, reports the result there. The output
/// never depends on `socket`: the context is used as is, and the sent report
/// gets [`add_estimates`].
pub fn compress_reported<'a>(
    input: &'a str,
    ctx: &CompressionContext,
    socket: Option<&Path>,
    adapter: Adapter,
    tool: ToolCategory,
) -> CompressionResult<'a> {
    let result = compress(input, ctx);
    if let Some(socket) = socket {
        let mut report = result.report.clone();
        add_estimates(&mut report, input, &result.output);
        send_report(socket, &report, adapter, tool);
    }
    result
}

/// Adds estimated token counts to a report that has none. Every transform
/// gates on the context's token counter, so adapters count after
/// [`compress`](deslag_core::compress) instead of adding a counter to the
/// context. `emitted` is the text the adapter keeps. The candidate count is
/// set only for `changed`, where the candidate is `emitted`.
pub fn add_estimates(report: &mut CompressionReport, input: &str, emitted: &str) {
    if report.original_tokens.is_some() {
        return;
    }
    let emitted = EstimateCounter.count(emitted);
    report.original_tokens = Some(EstimateCounter.count(input));
    report.emitted_tokens = Some(emitted);
    if report.outcome == Outcome::Changed {
        report.candidate_tokens = Some(emitted);
    }
}

/// Encodes and sends one report to `socket` without blocking. A failed send is
/// counted and carried in the next report's `dropped` field.
pub fn send_report(
    socket: &Path,
    report: &CompressionReport,
    adapter: Adapter,
    tool: ToolCategory,
) {
    // Swap, not load: concurrent senders never carry the same drops twice.
    let dropped = DROPPED.swap(0, Ordering::Relaxed);
    let message = encode(report, adapter, tool, dropped);
    if !send(socket, message.as_bytes()) {
        DROPPED.fetch_add(dropped + 1, Ordering::Relaxed);
    }
}
