//! Compression ownership and proxy-mode replay manifests.
//!
//! One owner selects the adapter that compresses: `hook`, `proxy` or `none`.
//! It lives in `<state dir>/owner` and is never inferred from tool-result
//! content. A missing, unreadable or unknown owner is `none`.
//!
//! Proxy mode pins one decision per `(session, tool_use_id)` in
//! `<state dir>/sessions/<sha256(session id)>.json` and replays it on every
//! later request, so resent history keeps its bytes. Each new decision,
//! `unchanged` included, is committed atomically before the request that
//! carries the result is forwarded. A manifest holds digests and decisions,
//! never tool content or the raw session id.

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::{Duration, SystemTime},
};

use ring::digest::{Context, SHA256, digest};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Hook,
    Proxy,
    None,
}

/// Mode for new sessions. A session keeps the mode it was created with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    Safe,
}

/// Manifests unchanged for this long are removed by [`prune`].
pub const RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

const VERSION: u64 = 1;
const POLICY_VERSION: u64 = 1;
const TRANSFORM: &str = "json-min";
const TRANSFORM_VERSION: u64 = 1;

/// `DESLAG_STATE_DIR`, else `$XDG_STATE_HOME/deslag`, else
/// `$HOME/.local/state/deslag`. Empty values count as unset.
pub fn state_dir() -> Option<PathBuf> {
    let var = |k| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    var("DESLAG_STATE_DIR")
        .or_else(|| var("XDG_STATE_HOME").map(|d| d.join("deslag")))
        .or_else(|| var("HOME").map(|d| d.join(".local/state/deslag")))
}

/// Atomically records the compression owner.
pub fn write_owner(dir: &Path, owner: Owner) -> io::Result<()> {
    let name = match owner {
        Owner::Hook => "hook",
        Owner::Proxy => "proxy",
        Owner::None => "none",
    };
    create_dir(dir)?;
    commit(&dir.join("owner"), format!("{name}\n").as_bytes())
}

pub fn read_owner(dir: &Path) -> Owner {
    match fs::read_to_string(dir.join("owner"))
        .as_deref()
        .map(str::trim_end)
    {
        Ok("hook") => Owner::Hook,
        Ok("proxy") => Owner::Proxy,
        _ => Owner::None,
    }
}

pub fn manifest_path(dir: &Path, scope: &str) -> PathBuf {
    let name = hex(digest(&SHA256, scope.as_bytes()).as_ref());
    dir.join("sessions").join(format!("{name}.json"))
}

/// Removes session manifests and leftover temp files last modified before
/// `now - retention`. Returns how many files were removed.
pub fn prune(dir: &Path, retention: Duration, now: SystemTime) -> io::Result<usize> {
    let cutoff = now.checked_sub(retention).unwrap_or(SystemTime::UNIX_EPOCH);
    let entries = match fs::read_dir(dir.join("sessions")) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        entries => entries?,
    };
    let mut removed = 0;
    for entry in entries {
        let path = entry?.path();
        let ext = path.extension().and_then(|e| e.to_str());
        if matches!(ext, Some("json" | "tmp")) && fs::metadata(&path)?.modified()? < cutoff {
            fs::remove_file(&path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[derive(Debug)]
pub enum ReplayError {
    /// The request carries tool results but the session has no readable manifest.
    MissingManifest,
    /// The manifest format version is not one this binary reads.
    UnsupportedManifest,
    /// A known tool_use_id arrived with different source text.
    DigestMismatch(String),
    /// A tool result outside the newest message was never decided.
    UndecidedHistory(String),
    /// A pinned decision names a transform id or version this binary does not keep.
    RetiredTransform(String),
    /// A pinned decision is neither `unchanged: true` nor `encoded` with a
    /// transform id and an unsigned integer version.
    CorruptDecision(String),
    /// A new decision could not be committed.
    Io(io::Error),
}

impl From<io::Error> for ReplayError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Replay state of one proxy: the state directory and the mode for new sessions.
#[derive(Clone, Debug)]
pub struct Store {
    pub dir: PathBuf,
    pub mode: Mode,
}

/// One tool result of a request, in document order.
pub struct Candidate<'a> {
    pub id: &'a str,
    /// The result is in the last message of the request.
    pub newest: bool,
    /// Decoded text targets; their digest identifies the source.
    pub texts: Vec<String>,
}

/// ponytail: one process-wide lock around load-decide-commit. Use per-scope
/// locks if concurrent sessions contend on it.
static LOCK: Mutex<()> = Mutex::new(());

impl Store {
    /// Returns, per candidate, whether json-min v1 applies. A known result
    /// gets its pinned decision. A new result in the newest message is
    /// decided by `decide` only while the configured owner is the proxy, and
    /// is otherwise `unchanged`. New decisions are committed before return.
    /// A request in a session the proxy does not own passes through.
    pub fn replay(
        &self,
        scope: &str,
        model: &str,
        candidates: &[Candidate],
        decide: impl Fn(&Candidate) -> bool,
    ) -> Result<Vec<bool>, ReplayError> {
        let _guard = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let owner = read_owner(&self.dir);
        let path = manifest_path(&self.dir, scope);
        let has_results = !candidates.is_empty();
        let pass = vec![false; candidates.len()];
        let mut manifest = match fs::read(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                if owner != Owner::Proxy {
                    return Ok(pass);
                }
                // History without a manifest may have been encoded; never adopt it.
                if has_results {
                    return Err(ReplayError::MissingManifest);
                }
                let mode = if self.mode == Mode::Off {
                    "off"
                } else {
                    "safe"
                };
                let fresh = json!({"version": VERSION, "owner": "proxy", "mode": mode,
                    "model_profile": model, "policy_version": POLICY_VERSION, "decisions": {}});
                write_manifest(&path, &fresh)?;
                return Ok(pass);
            }
            read => {
                let parsed = read
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
                match parsed.filter(|m| m["decisions"].is_object()) {
                    Some(m) => m,
                    None if has_results => return Err(ReplayError::MissingManifest),
                    // A corrupt manifest is never overwritten.
                    None => return Ok(pass),
                }
            }
        };
        if manifest["version"] != VERSION {
            return if has_results {
                Err(ReplayError::UnsupportedManifest)
            } else {
                Ok(pass)
            };
        }
        // Owner and mode are pinned per session.
        if manifest["owner"] != "proxy" || manifest["mode"] != "safe" {
            return Ok(pass);
        }
        let decisions = &mut manifest["decisions"];
        let mut encode = Vec::with_capacity(candidates.len());
        let mut dirty = false;
        for c in candidates {
            let source = text_digest(&c.texts);
            let pinned = match decisions.get(c.id) {
                Some(d) if d["digest"] != source.as_str() => {
                    return Err(ReplayError::DigestMismatch(c.id.into()));
                }
                Some(d) => pinned(d, c.id)?,
                None if !c.newest => return Err(ReplayError::UndecidedHistory(c.id.into())),
                None => {
                    let enc = owner == Owner::Proxy && decide(c);
                    let mut d = json!({"digest": source, "policy_version": POLICY_VERSION,
                        "model_profile": model});
                    if enc {
                        d["encoded"] =
                            json!({"transform": TRANSFORM, "version": TRANSFORM_VERSION});
                    } else {
                        d["unchanged"] = true.into();
                    }
                    decisions[c.id] = d;
                    dirty = true;
                    enc
                }
            };
            encode.push(pinned);
        }
        if dirty {
            write_manifest(&path, &manifest)?;
        }
        Ok(encode)
    }
}

/// Whether a pinned decision encodes. Only `unchanged: true` or `encoded` with
/// a transform id and an unsigned integer version is valid; any other shape
/// is an error, never a silent replay as unchanged.
fn pinned(d: &Value, id: &str) -> Result<bool, ReplayError> {
    match (d.get("unchanged"), d.get("encoded")) {
        (Some(Value::Bool(true)), None) => Ok(false),
        (None, Some(e)) => match (e["transform"].as_str(), e["version"].as_u64()) {
            (Some(TRANSFORM), Some(TRANSFORM_VERSION)) => Ok(true),
            (Some(_), Some(_)) => Err(ReplayError::RetiredTransform(id.into())),
            _ => Err(ReplayError::CorruptDecision(id.into())),
        },
        _ => Err(ReplayError::CorruptDecision(id.into())),
    }
}

/// SHA-256 over length-prefixed texts, so target boundaries are part of the digest.
fn text_digest(texts: &[String]) -> String {
    let mut ctx = Context::new(&SHA256);
    for t in texts {
        ctx.update(&(t.len() as u64).to_le_bytes());
        ctx.update(t.as_bytes());
    }
    hex(ctx.finish().as_ref())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn write_manifest(path: &Path, manifest: &Value) -> io::Result<()> {
    create_dir(path.parent().expect("manifest path has a parent"))?;
    commit(path, manifest.to_string().as_bytes())
}

/// Owner-only directory on unix.
fn create_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

/// Writes a sibling temp file, syncs it, renames it over `path` and syncs the
/// directory, so a reader sees the old or the new content, never a mix.
fn commit(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    // A leftover from a crashed commit may carry other permissions.
    let _ = fs::remove_file(&tmp);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&tmp, path)?;
    #[cfg(unix)]
    fs::File::open(path.parent().expect("state path has a parent"))?.sync_all()?;
    Ok(())
}
