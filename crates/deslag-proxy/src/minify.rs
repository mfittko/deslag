//! Opt-in JSON minification of Bash and MCP tool results in Messages requests.
//!
//! A `tool_result` is eligible when its `tool_use_id` names a `tool_use` of
//! `Bash` or of an `mcp__*` tool and it is not `"is_error": true`. Only its
//! string `content` and the `text` of its `"type": "text"` content items are
//! approved for change. Every other block and field, including `thinking` and
//! its `signature`, `redacted_thinking`, `cache_control`, `is_error` and
//! unknown types and fields, stays byte-identical. An ineligible new result is
//! recorded as `unchanged`. The eligibility rule decides new results; results
//! already pinned in a session manifest, including those recorded under an
//! earlier policy version, replay as recorded. Local reports carry the tool
//! category of the result.
//!
//! The request is never re-serialized. Each rewritten string lexeme is located
//! by its byte span in the original body and spliced; all other bytes are
//! forwarded unchanged.
//!
//! Only requests with an `x-claude-code-session-id` header are rewritten. New
//! tool results are decided only while the owner is `proxy`; under any other
//! owner they are recorded as `unchanged`. Every tool result gets one
//! decision, `unchanged` or json-minify, pinned in the session manifest (see
//! `session`) before the request is forwarded. Resent history replays its
//! pinned decision under any owner, so its bytes upstream stay the same across
//! resend, restart, disable and model switch. Missing or inconsistent replay
//! state rejects the request with `400` and is never forwarded, while the
//! owner is `proxy` or the session has a manifest. Requests with
//! `content-encoding`, a body-integrity header, trailers or a body over
//! `MAX_INSPECT_BYTES` pass through unreplayed: their pinned encoded results go
//! out in original form, which drifts the prefix of that request. They are
//! unrecorded, so their new tool results stay undecided. A later request in
//! that session that resends them fails with `400`; start a new session.
//! Trailers are not forwarded upstream. `/v1/messages/count_tokens` is not
//! rewritten, so its counts can overstate.
//!
//! Only a new tool result is decided under `Limits::executor`: each of its
//! texts, a text item of a content array or the string content, runs as one
//! job. When the executor rejects any job of that result (busy, budget exceeded
//! or panicked), the result is recorded as `unchanged`. A result pinned as
//! json-minify is re-encoded outside the executor, so a rejection never
//! changes the bytes of already-sent history.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    path::Path,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use deslag_core::{BoundedExecutor, CompressionReport, FIRST_SLICE_JSON_MINIFY_CONTEXT, compress};
use deslag_telemetry as local;
use http_body_util::BodyExt;
use hyper::{
    HeaderMap, Method, Request, StatusCode,
    body::{Body, Frame, Incoming, SizeHint},
    header,
    http::request::Parts,
};
use serde::de::{Deserialize, Deserializer, Error, MapAccess, Visitor};
use serde_json::{Value, value::RawValue};
use tokio::{
    sync::{Semaphore, SemaphorePermit},
    time::Duration,
};

use crate::session::{Candidate, ReplayError, Store};

/// ponytail: at or above the Anthropic Messages request limit (32 MB), so every
/// request the provider accepts is inspected and the decision never depends on
/// size. A lower cap would bypass long sessions and change their history bytes.
/// A larger body is forwarded unrewritten: the buffered prefix, then the rest.
const MAX_INSPECT_BYTES: usize = 32 * 1024 * 1024;

/// ponytail: at most 4 requests buffer and rewrite at once, so peak inspection
/// memory is about 4 x ~100 MiB (buffer growth plus rewrite output). Others wait
/// for a permit and never stream through, so the decision never depends on load.
/// A permit holder that does not deliver its body within
/// `Limits::body_read_timeout` gets `408` and releases the permit.
/// The deadline covers only the buffered part, up to `MAX_INSPECT_BYTES`, and
/// never the replay. The permit is held until the replay ends. For a larger
/// body the permit is released and the rest streams without a deadline. The
/// body parse, replay and rebuild have no time limit. The jobs of new results
/// run in sequence, and the budget of `Limits::executor` bounds each job only.
/// Make the permit count configurable via `Limits` if that ceiling is wrong.
static INSPECT_PERMITS: Semaphore = Semaphore::const_new(4);

/// Free inspection permits. Test readiness hook only.
#[doc(hidden)]
pub fn available_inspect_permits() -> usize {
    INSPECT_PERMITS.available_permits()
}

/// Headers that pin the exact body bytes; rewriting would break the check.
const INTEGRITY_HEADERS: [&str; 5] = [
    "content-digest",
    "digest",
    "content-md5",
    "x-amz-content-sha256",
    "repr-digest",
];

/// Request body: an optional buffered prefix followed by the rest of the original
/// body, or by the buffered trailers when the body was read to the end. The
/// upstream connection drops request trailers; see [`buffer`].
pub(crate) struct ReqBody {
    prefix: Option<Bytes>,
    rest: Option<Incoming>,
    trailers: Option<HeaderMap>,
}

impl Body for ReqBody {
    type Data = Bytes;
    type Error = hyper::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, hyper::Error>>> {
        let this = self.get_mut();
        if let Some(prefix) = this.prefix.take().filter(|p| !p.is_empty()) {
            return Poll::Ready(Some(Ok(Frame::data(prefix))));
        }
        match this.rest.as_mut() {
            Some(rest) => Pin::new(rest).poll_frame(cx),
            None => Poll::Ready(this.trailers.take().map(|t| Ok(Frame::trailers(t)))),
        }
    }

    fn is_end_stream(&self) -> bool {
        self.prefix.as_ref().is_none_or(Bytes::is_empty)
            && self.rest.as_ref().is_none_or(Body::is_end_stream)
            && self.trailers.is_none()
    }

    fn size_hint(&self) -> SizeHint {
        let len = self.prefix.as_ref().map_or(0, |p| p.len() as u64);
        let Some(rest) = &self.rest else {
            return SizeHint::with_exact(len);
        };
        let rest = rest.size_hint();
        let mut hint = SizeHint::new();
        hint.set_lower(rest.lower() + len);
        if let Some(upper) = rest.upper() {
            hint.set_upper(upper + len);
        }
        hint
    }
}

/// Status, error type and message of a request the proxy answers itself.
pub(crate) type Reject = (StatusCode, &'static str, &'static str);

const READ_FAILED: &str = "deslag: request body read failed";
const STATE_FAILED: Reject = (
    StatusCode::INTERNAL_SERVER_ERROR,
    "api_error",
    "deslag: session state read or write failed",
);
const SCOPE_HEADER: &str = "x-claude-code-session-id";

/// Returns the request to forward. With a `store`, an uncompressed
/// `POST /v1/messages` body with a non-empty session id is buffered and its tool results replayed.
/// Anything else streams through untouched. A failed buffered read is `400`.
/// A buffered read that exceeds `read_timeout` after its permit is granted is
/// `408`: the request is rejected, never forwarded unrewritten. The timeout
/// covers only the body read, never the replay. The replay holds the permit
/// until it ends, also when the client disconnects mid-replay. Replay errors
/// are `400`; a failed state commit or a panicked replay is `500`. Neither is
/// forwarded.
pub(crate) async fn prepare(
    req: Request<Incoming>,
    store: Option<&Store>,
    read_timeout: Duration,
    exec: BoundedExecutor,
) -> Result<Request<ReqBody>, Reject> {
    // Only a scoped request is replayed, so a request without a session id
    // streams through and never takes an inspection permit.
    let eligible = req.method() == Method::POST
        && req.uri().path() == "/v1/messages"
        && scope_of(req.headers()).is_some()
        && !req.headers().contains_key(header::CONTENT_ENCODING)
        && !INTEGRITY_HEADERS
            .iter()
            .any(|h| req.headers().contains_key(*h));
    let (parts, body) = req.into_parts();
    let Some(store) = store.filter(|_| eligible) else {
        let body = ReqBody {
            prefix: None,
            rest: Some(body),
            trailers: None,
        };
        return Ok(Request::from_parts(parts, body));
    };
    let permit = INSPECT_PERMITS
        .acquire()
        .await
        .expect("semaphore is never closed");
    // The deadline starts at the permit, so time spent queued does not count.
    // It covers only the body read, so a state stall is never reported as `408`.
    let (mut parts, buf, trailers) =
        match tokio::time::timeout(read_timeout, buffer(parts, body)).await {
            Err(_) => {
                return Err((
                    StatusCode::REQUEST_TIMEOUT,
                    "invalid_request_error",
                    READ_FAILED,
                ));
            }
            Ok(Err(e)) => return Err(e),
            Ok(Ok(Buffered::Overflow(req))) => return Ok(req),
            Ok(Ok(Buffered::Complete(parts, buf, trailers))) => (parts, buf, trailers),
        };
    let mut buf = Bytes::from(buf);
    if let (Some(scope), None) = (scope_of(&parts.headers), &trailers) {
        let input = buf.clone();
        let (store, scope) = (store.clone(), scope.to_owned());
        if let Some(rewritten) = rewrite_blocking(permit, input, store, scope, exec, minify).await?
        {
            buf = rewritten.into();
            parts
                .headers
                .insert(header::CONTENT_LENGTH, buf.len().into());
        }
    }
    let body = ReqBody {
        prefix: Some(buf),
        rest: None,
        trailers,
    };
    Ok(Request::from_parts(parts, body))
}

/// Runs [`rewrite`] on the blocking pool, outside the read deadline, because
/// manifest IO blocks. The permit moves into the blocking task, so the buffer
/// stays counted against `INSPECT_PERMITS` until the replay ends, even when
/// the caller is dropped (client disconnect). A replay error is `400`; a failed
/// state commit or a panic is `500`. The telemetry socket lookup (one `stat`)
/// and the report sends run here too.
async fn rewrite_blocking(
    permit: SemaphorePermit<'static>,
    input: Bytes,
    store: Store,
    scope: String,
    exec: BoundedExecutor,
    job: Job,
) -> Result<Option<Vec<u8>>, Reject> {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let socket = local::live_socket();
        rewrite(&input, &store, &scope, &exec, job, socket.as_deref())
    })
    .await
    .map_err(|_| STATE_FAILED)?
    .map_err(reject)
}

/// The non-empty session id, or `None`.
fn scope_of(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(SCOPE_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
}

/// A buffered request body.
enum Buffered {
    /// The body exceeded `MAX_INSPECT_BYTES`; forward it unrewritten.
    Overflow(Request<ReqBody>),
    /// The whole body and its trailers, ready for replay.
    Complete(Parts, Vec<u8>, Option<HeaderMap>),
}

/// Buffers up to `MAX_INSPECT_BYTES` of `body`. A body with trailers is never
/// rewritten, because a trailer such as `content-digest` can pin its bytes. The
/// trailers are kept in the body, but they do not reach the upstream: the
/// hop-by-hop strip removes the `trailer` header, and a `content-length` body
/// carries no trailers over HTTP/1.1.
async fn buffer(parts: Parts, mut body: Incoming) -> Result<Buffered, Reject> {
    let hint = body.size_hint().exact().unwrap_or(0);
    let mut buf = Vec::with_capacity(hint.min(MAX_INSPECT_BYTES as u64 + 1) as usize);
    let mut trailers: Option<HeaderMap> = None;
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                READ_FAILED,
            )
        })?;
        let data = match frame.into_data() {
            Ok(data) => data,
            Err(frame) => {
                if let Ok(t) = frame.into_trailers() {
                    trailers.get_or_insert_default().extend(t);
                }
                continue;
            }
        };
        buf.extend_from_slice(&data);
        if buf.len() > MAX_INSPECT_BYTES {
            // Never truncate or reject: forward the prefix, then the rest as it arrives.
            let body = ReqBody {
                prefix: Some(buf.into()),
                rest: Some(body),
                trailers: None,
            };
            return Ok(Buffered::Overflow(Request::from_parts(parts, body)));
        }
    }
    Ok(Buffered::Complete(parts, buf, trailers))
}

fn reject(e: ReplayError) -> Reject {
    let message = match e {
        ReplayError::Io(_) => return STATE_FAILED,
        ReplayError::MissingManifest => {
            "deslag: no replay state for this session history; start a new session"
        }
        ReplayError::UnsupportedManifest => {
            "deslag: unsupported replay state version; start a new session"
        }
        ReplayError::DigestMismatch(_) => "deslag: a sent tool result changed; start a new session",
        ReplayError::UndecidedHistory(_) => {
            "deslag: history holds a tool result with no recorded decision; start a new session"
        }
        ReplayError::RetiredTransform(_) => {
            "deslag: a pinned transform is not available in this version; start a new session"
        }
        ReplayError::CorruptDecision(_) => {
            "deslag: replay state holds an invalid decision; start a new session"
        }
    };
    (StatusCode::BAD_REQUEST, "invalid_request_error", message)
}

/// One finished job: the changed text, if any, and the compression report.
type Compressed = (Option<String>, CompressionReport);

/// Per-text compression job. Tests inject a slow or panicking one.
type Job = fn(&str) -> Compressed;

fn minify(text: &str) -> Compressed {
    let result = compress(text, &FIRST_SLICE_JSON_MINIFY_CONTEXT);
    let report = result.report.clone();
    (result.into_changed(), report)
}

/// Replays the session's decisions on the tool results and splices `job`
/// output where they encode. `Ok(None)` forwards the body unchanged. With
/// `socket`, every spliced block is reported there. History resends repeat
/// blocks, so the proxy counts submissions, not unique results.
fn rewrite(
    body: &[u8],
    store: &Store,
    scope: &str,
    exec: &BoundedExecutor,
    job: Job,
    socket: Option<&Path>,
) -> Result<Option<Vec<u8>>, ReplayError> {
    let Some((model, results)) = tool_results(body) else {
        return Ok(None);
    };
    let candidates: Vec<Candidate> = results
        .iter()
        .map(|r| Candidate {
            id: r.id,
            newest: r.newest,
            source: canonical(r.content),
            texts: r.targets.iter().filter_map(|t| parse(t)).collect(),
        })
        .collect();
    // Fail closed: one ineligible result makes every result with its id
    // ineligible, because they share one pinned decision.
    let ineligible: HashSet<&str> = results
        .iter()
        .filter(|r| r.tool == local::ToolCategory::Other || r.is_error)
        .map(|r| r.id)
        .collect();
    // `decide` runs only for a new result. Only an eligible result whose text
    // json-minify changes is encoded.
    let encode = store.replay(scope, model, &candidates, |c| {
        !ineligible.contains(c.id) && changes(&c.texts, exec, job)
    })?;
    let targets = results
        .iter()
        .zip(encode)
        .filter(|(_, encode)| *encode)
        .flat_map(|(r, _)| r.targets.iter().map(|t| (*t, r.tool)));
    Ok(splice(body, targets, job, socket))
}

/// Whether `job` changes any of `texts`. Each text runs as one job of `exec`.
/// Any rejected job (busy, budget exceeded or panicked) returns `false`, so the
/// new result is recorded as `unchanged`.
fn changes(texts: &[String], exec: &BoundedExecutor, job: Job) -> bool {
    let mut changed = false;
    for text in texts {
        let text = text.clone();
        match exec.run(move || job(&text).0.is_some()) {
            Ok(c) => changed |= c,
            Err(_) => return false,
        }
    }
    changed
}

/// The complete tool-result content as compact JSON with sorted keys, so every
/// block, text or not, is part of the source identity and whitespace and
/// escapes are not. `cache_control` on a content block is excluded, so a moved
/// cache breakpoint is not a changed result. A missing content is the empty
/// string.
fn canonical(content: Option<&RawValue>) -> String {
    content.map_or_else(String::new, |c| {
        parse::<Value>(c).map_or_else(
            || c.get().to_owned(),
            |mut v| {
                if let Some(blocks) = v.as_array_mut() {
                    for block in blocks.iter_mut().filter_map(Value::as_object_mut) {
                        block.remove("cache_control");
                    }
                }
                v.to_string()
            },
        )
    })
}

/// One `tool_result` block of a user message.
struct ToolResult<'a> {
    id: &'a str,
    /// The block is in the last message.
    newest: bool,
    content: Option<&'a RawValue>,
    /// The approved string fields, in document order.
    targets: Vec<&'a RawValue>,
    /// Category of the `tool_use` that `id` names; `Other` when none does.
    tool: local::ToolCategory,
    /// `"is_error": true`.
    is_error: bool,
}

/// `None` when the body is not a JSON object with a non-empty `messages` array.
fn tool_results(body: &[u8]) -> Option<(&str, Vec<ToolResult<'_>>)> {
    let root: Obj = serde_json::from_slice(body).ok()?;
    let raw = parse::<Vec<&RawValue>>(root.get("messages")?)?;
    let last = raw.len().checked_sub(1)?;
    let messages: Vec<(usize, Obj)> = raw
        .into_iter()
        .enumerate()
        .filter_map(|(i, m)| Some((i, parse(m)?)))
        .collect();
    let blocks = |role: &'static str| {
        messages
            .iter()
            .filter(move |(_, m)| m.str("role") == Some(role))
            .filter_map(|(i, m)| Some((*i, parse::<Vec<&RawValue>>(m.get("content")?)?)))
            .flat_map(|(i, blocks)| {
                blocks
                    .into_iter()
                    .filter_map(parse::<Obj>)
                    .map(move |b| (i, b))
            })
    };
    // An id used by tool uses of different categories is `Other`: fail closed.
    // A missing, non-string or escaped name is `Other` as well.
    let mut tools: HashMap<&str, local::ToolCategory> = HashMap::new();
    for (id, name) in blocks("assistant")
        .filter(|(_, b)| b.str("type") == Some("tool_use"))
        .filter_map(|(_, b)| Some((b.str("id")?, b.str("name"))))
    {
        let tool = name.map_or(
            local::ToolCategory::Other,
            local::ToolCategory::from_tool_name,
        );
        tools
            .entry(id)
            .and_modify(|t| {
                if *t != tool {
                    *t = local::ToolCategory::Other;
                }
            })
            .or_insert(tool);
    }
    let mut results = Vec::new();
    for (i, block) in blocks("user") {
        let (Some("tool_result"), Some(id)) = (block.str("type"), block.str("tool_use_id")) else {
            continue;
        };
        let content = block.get("content");
        let targets = match content {
            None => Vec::new(),
            Some(content) => match parse::<Vec<&RawValue>>(content) {
                Some(items) => items
                    .into_iter()
                    .filter_map(parse::<Obj>)
                    .filter(|item| item.str("type") == Some("text"))
                    .filter_map(|item| item.get("text"))
                    .collect(),
                None => vec![content],
            },
        };
        results.push(ToolResult {
            id,
            newest: i == last,
            content,
            targets,
            tool: tools.get(id).copied().unwrap_or(local::ToolCategory::Other),
            is_error: block.get("is_error").and_then(parse) == Some(true),
        });
    }
    Some((root.str("model").unwrap_or_default(), results))
}

/// Replaces each string target that `job` changes. `None` when nothing changed.
/// `job` runs directly, never under the executor: a pinned json-minify
/// decision must re-encode to the same bytes on every resend, so it can never
/// fail open.
fn splice<'a>(
    body: &[u8],
    targets: impl Iterator<Item = (&'a RawValue, local::ToolCategory)>,
    job: Job,
    socket: Option<&Path>,
) -> Option<Vec<u8>> {
    // Targets are in document order, so spans are increasing and disjoint.
    let mut out = Vec::with_capacity(body.len());
    let mut pos = 0;
    let mut reports = Vec::new();
    for (raw, tool) in targets {
        let Some(text) = parse::<String>(raw) else {
            continue;
        };
        let (changed, mut compressed) = job(&text);
        if socket.is_some() {
            local::add_estimates(&mut compressed, &text, changed.as_deref().unwrap_or(&text));
            reports.push((compressed, tool));
        }
        let Some(minified) = changed else {
            continue;
        };
        let start = (raw.get().as_ptr() as usize).checked_sub(body.as_ptr() as usize)?;
        if start < pos || start + raw.get().len() > body.len() {
            return None;
        }
        out.extend_from_slice(&body[pos..start]);
        out.extend_from_slice(serde_json::to_string(&minified).ok()?.as_bytes());
        pos = start + raw.get().len();
    }
    if let Some(socket) = socket {
        for (compressed, tool) in &reports {
            // ponytail: no session. Every submission resends the whole history,
            // so per-request attribution from here would count a result once
            // per later request; attributing new blocks only needs replay state.
            local::send_report(socket, compressed, local::Adapter::Proxy, *tool, None);
        }
    }
    if pos == 0 {
        return None;
    }
    out.extend_from_slice(&body[pos..]);
    Some(out)
}

fn parse<'a, T: Deserialize<'a>>(raw: &'a RawValue) -> Option<T> {
    serde_json::from_str(raw.get()).ok()
}

/// One JSON object's members as borrowed raw values, in order. Parsing fails on
/// duplicate or escaped keys, so such an object is skipped. Only the object's
/// own keys are checked, never nested ones, which keeps each decision local.
struct Obj<'a>(Vec<(&'a str, &'a RawValue)>);

impl<'a> Obj<'a> {
    fn get(&self, key: &str) -> Option<&'a RawValue> {
        self.0.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    }

    /// The member as an unescaped string; escaped values never match.
    fn str(&self, key: &str) -> Option<&'a str> {
        parse(self.get(key)?)
    }
}

impl<'de> Deserialize<'de> for Obj<'de> {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct ObjVisitor;
        impl<'de> Visitor<'de> for ObjVisitor {
            type Value = Obj<'de>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Obj<'de>, A::Error> {
                let mut members: Vec<(&str, &RawValue)> = Vec::new();
                // A borrowed &str key fails on any escape, which also rules out
                // escape-equivalent duplicates.
                while let Some(key) = map.next_key::<&str>()? {
                    // ponytail: linear duplicate scan; content blocks have a handful of keys.
                    if members.iter().any(|(k, _)| *k == key) {
                        return Err(A::Error::custom("duplicate key"));
                    }
                    members.push((key, map.next_value()?));
                }
                Ok(Obj(members))
            }
        }
        de.deserialize_map(ObjVisitor)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use deslag_core::BoundedExecutor;
    use http_body_util::BodyExt;
    use hyper::HeaderMap;

    use super::{ReqBody, minify, rewrite, rewrite_blocking};
    use crate::session::{Mode, Owner, Store, write_owner};

    /// A budget no debug build on a slow runner can miss.
    fn generous() -> BoundedExecutor {
        BoundedExecutor::new(4, Duration::from_secs(60)).unwrap()
    }

    /// Minifies, but far slower than any budget the tests below use for it.
    fn slow_on_b(text: &str) -> super::Compressed {
        if text.contains("\"b\"") {
            std::thread::sleep(Duration::from_secs(10));
        }
        minify(text)
    }

    fn panic_on_b(text: &str) -> super::Compressed {
        assert!(!text.contains("\"b\""), "injected panic");
        minify(text)
    }

    /// Two Bash tool results; the second one holds key `b`.
    const TWO_RESULTS: &str = r#"{"messages":[
{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}},{"type":"tool_use","id":"t2","name":"Bash","input":{}}]},
{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"{\n  \"a\": 1\n}"},{"type":"tool_result","tool_use_id":"t2","content":"{\n  \"b\": 1\n}"}]}]}"#;

    /// `TWO_RESULTS` with only the first result minified.
    fn only_a_minified() -> String {
        TWO_RESULTS.replace(r#""{\n  \"a\": 1\n}""#, r#""{\"a\":1}""#)
    }

    /// A proxy-owned store with an empty manifest for scope `s`.
    fn fresh(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("deslag-minify-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_owner(&dir, Owner::Proxy).unwrap();
        let store = Store {
            dir,
            mode: Mode::Safe,
        };
        let first = br#"{"messages":[{"role":"user","content":"hi"}]}"#;
        assert!(matches!(
            rewrite(first, &store, "s", &generous(), minify, None),
            Ok(None)
        ));
        store
    }

    /// An executor whose only permit is held until the sender is dropped.
    fn busy() -> (
        BoundedExecutor,
        std::sync::mpsc::Sender<()>,
        std::thread::JoinHandle<bool>,
    ) {
        let exec = BoundedExecutor::new(1, Duration::from_secs(60)).unwrap();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let holder = {
            let exec = exec.clone();
            std::thread::spawn(move || exec.run(move || wait.recv()).is_ok())
        };
        while exec.in_flight() == 0 {
            std::thread::yield_now();
        }
        (exec, release, holder)
    }

    #[tokio::test]
    async fn buffered_body_replays_its_trailers() {
        let mut trailers = HeaderMap::new();
        trailers.insert("content-digest", "sha-256=:x:".parse().unwrap());
        let body = ReqBody {
            prefix: Some("{}".into()),
            rest: None,
            trailers: Some(trailers.clone()),
        };
        let collected = body.collect().await.unwrap();
        assert_eq!(collected.trailers(), Some(&trailers));
        assert_eq!(collected.to_bytes(), "{}");
    }

    #[test]
    fn rewrite_skips_unchanged_and_invalid_bodies() {
        for body in [
            &b""[..],
            b"{",
            b"[]",
            br#"{"messages":[]}"#,
            br#"{"messages":[{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"[ 1 ]"}]}]}"#,
        ] {
            let store = Store {
                dir: std::env::temp_dir().join("deslag-no-such-state-dir"),
                mode: Mode::Safe,
            };
            assert!(matches!(
                rewrite(body, &store, "s", &generous(), minify, None),
                Ok(None)
            ));
        }
    }

    #[test]
    fn rewrite_minifies_every_target() {
        let store = fresh("every");
        let expected = only_a_minified().replace(r#""{\n  \"b\": 1\n}""#, r#""{\"b\":1}""#);
        let out = rewrite(
            TWO_RESULTS.as_bytes(),
            &store,
            "s",
            &generous(),
            minify,
            None,
        );
        assert_eq!(String::from_utf8(out.unwrap().unwrap()).unwrap(), expected);
        let _ = std::fs::remove_dir_all(&store.dir);
    }

    #[test]
    fn duplicate_ids_fail_closed() {
        // An error result shares its id with an earlier non-error result.
        let error_dup = r#"{"messages":[
{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]},
{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"[ 1 ]"},{"type":"tool_result","tool_use_id":"t","content":"[ 1 ]","is_error":true}]}]}"#;
        // Two tool uses with one id and different categories, Bash last.
        let tool_dup = r#"{"messages":[
{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Read","input":{}},{"type":"tool_use","id":"t","name":"Bash","input":{}}]},
{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"[ 1 ]"}]}]}"#;
        for (name, body) in [("dup-error", error_dup), ("dup-tool", tool_dup)] {
            let store = fresh(name);
            let out = rewrite(body.as_bytes(), &store, "s", &generous(), minify, None);
            assert!(matches!(out, Ok(None)), "{name}");
            let _ = std::fs::remove_dir_all(&store.dir);
        }
    }

    #[test]
    fn tool_use_names_decide_eligibility() {
        let bash = r#"{"type":"tool_use","id":"t","name":"Bash","input":{}}"#;
        let body = |uses: &str| {
            format!(
                r#"{{"messages":[{{"role":"assistant","content":[{uses}]}},{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t","content":"[ 1 ]"}}]}}]}}"#
            )
        };
        let second = |rest: &str| {
            body(&format!(
                r#"{bash},{{"type":"tool_use","id":"t"{rest},"input":{{}}}}"#
            ))
        };
        let cases = [
            ("bash", body(bash), true),
            (
                "mcp",
                body(r#"{"type":"tool_use","id":"t","name":"mcp__server__tool","input":{}}"#),
                true,
            ),
            ("bash-bash", body(&format!("{bash},{bash}")), true),
            // A second use with a conflicting or unreadable name fails closed.
            ("bash-escaped", second(r#","name":"Read""#), false),
            ("bash-no-name", second(""), false),
            ("bash-number", second(r#","name":5"#), false),
            ("bash-read", second(r#","name":"Read""#), false),
        ];
        for (name, body, minified) in cases {
            let store = fresh(&format!("names-{name}"));
            let out = rewrite(body.as_bytes(), &store, "s", &generous(), minify, None);
            assert_eq!(out.unwrap().is_some(), minified, "{name}");
            let _ = std::fs::remove_dir_all(&store.dir);
        }
    }

    #[test]
    fn expired_job_records_its_result_unchanged() {
        let store = fresh("expired");
        let exec = BoundedExecutor::new(1, Duration::from_millis(200)).unwrap();
        let out = rewrite(TWO_RESULTS.as_bytes(), &store, "s", &exec, slow_on_b, None);
        assert_eq!(out.unwrap().unwrap(), only_a_minified().into_bytes());
        // The resend replays `unchanged` for `t2`, even with a free executor.
        let out = rewrite(
            TWO_RESULTS.as_bytes(),
            &store,
            "s",
            &generous(),
            minify,
            None,
        );
        assert_eq!(out.unwrap().unwrap(), only_a_minified().into_bytes());
        let _ = std::fs::remove_dir_all(&store.dir);
    }

    #[test]
    fn panicked_job_records_its_result_unchanged() {
        let store = fresh("panicked");
        let out = rewrite(
            TWO_RESULTS.as_bytes(),
            &store,
            "s",
            &generous(),
            panic_on_b,
            None,
        );
        assert_eq!(out.unwrap().unwrap(), only_a_minified().into_bytes());
        let out = rewrite(
            TWO_RESULTS.as_bytes(),
            &store,
            "s",
            &generous(),
            minify,
            None,
        );
        assert_eq!(out.unwrap().unwrap(), only_a_minified().into_bytes());
        let _ = std::fs::remove_dir_all(&store.dir);
    }

    #[test]
    fn busy_executor_records_new_results_unchanged() {
        let store = fresh("busy");
        let (exec, release, holder) = busy();
        let out = rewrite(TWO_RESULTS.as_bytes(), &store, "s", &exec, minify, None);
        assert!(matches!(out, Ok(None)));
        drop(release);
        assert!(holder.join().unwrap());
        // Pinned `unchanged` stays unchanged once the executor is free.
        let out = rewrite(TWO_RESULTS.as_bytes(), &store, "s", &exec, minify, None);
        assert!(matches!(out, Ok(None)));
        let _ = std::fs::remove_dir_all(&store.dir);
    }

    #[test]
    fn pinned_encoded_result_never_fails_open() {
        let store = fresh("pinned");
        let first = rewrite(
            TWO_RESULTS.as_bytes(),
            &store,
            "s",
            &generous(),
            minify,
            None,
        );
        let first = first.unwrap().expect("minified");
        // A busy executor rejects every job, but replay re-encodes outside it.
        let (exec, release, holder) = busy();
        let out = rewrite(TWO_RESULTS.as_bytes(), &store, "s", &exec, minify, None);
        assert_eq!(out.unwrap(), Some(first));
        drop(release);
        assert!(holder.join().unwrap());
        let _ = std::fs::remove_dir_all(&store.dir);
    }

    #[tokio::test]
    async fn dropped_rewrite_keeps_its_permit_until_the_rewrite_ends() {
        static PERMITS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
        static OPEN: AtomicBool = AtomicBool::new(false);
        /// Blocks until the test opens the gate.
        fn gated(text: &str) -> super::Compressed {
            while !OPEN.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            minify(text)
        }
        let store = fresh("dropped");
        let permit = PERMITS.acquire().await.unwrap();
        let input = bytes::Bytes::from_static(TWO_RESULTS.as_bytes());
        // Polls the rewrite until it is spawned, then drops it like a disconnect.
        let fut = rewrite_blocking(permit, input, store.clone(), "s".into(), generous(), gated);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), fut)
                .await
                .is_err()
        );
        assert_eq!(PERMITS.available_permits(), 0, "permit freed mid-rewrite");
        OPEN.store(true, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(60), async {
            while PERMITS.available_permits() == 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("permit never released after the rewrite ended");
        let _ = std::fs::remove_dir_all(&store.dir);
    }

    #[test]
    fn rewrite_output_does_not_depend_on_the_service() {
        use std::os::unix::net::UnixDatagram;
        let store = fresh("service");
        let body = br#"{"messages":[{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash"}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"[ 1 ]"}]}]}"#;
        let run = |socket: Option<&std::path::Path>| {
            rewrite(body, &store, "s", &generous(), minify, socket).unwrap()
        };
        let plain = run(None).expect("minified");
        let dir = std::path::PathBuf::from(format!("/tmp/dsl-proxy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.sock");
        // A stale socket file: the service is gone.
        drop(UnixDatagram::bind(&path).unwrap());
        assert_eq!(run(Some(&path)).as_ref(), Some(&plain));
        std::fs::remove_file(&path).unwrap();
        let receiver = UnixDatagram::bind(&path).unwrap();
        receiver.set_nonblocking(true).unwrap();
        assert_eq!(run(Some(&path)).as_ref(), Some(&plain));
        let mut buf = [0; 1024];
        let n = receiver.recv(&mut buf).unwrap();
        let report: serde_json::Value = serde_json::from_slice(&buf[..n]).unwrap();
        assert_eq!(report["adapter"], "proxy");
        assert_eq!(report["outcome"], "changed");
        assert_eq!(report["tool"], "bash");
        assert!(receiver.recv(&mut buf).is_err(), "one block, one report");
        // An MCP result reports its own category.
        let mcp = fresh("service-mcp");
        let body = String::from_utf8(body.to_vec()).unwrap();
        let body = body.replace(r#""name":"Bash""#, r#""name":"mcp__s__t""#);
        let out = rewrite(body.as_bytes(), &mcp, "s", &generous(), minify, Some(&path));
        assert!(out.unwrap().is_some());
        let n = receiver.recv(&mut buf).unwrap();
        let report: serde_json::Value = serde_json::from_slice(&buf[..n]).unwrap();
        assert_eq!(report["tool"], "mcp");
        let _ = std::fs::remove_dir_all(&mcp.dir);
        // A result whose job panicked is recorded `unchanged` and not reported.
        let other = fresh("service-panic");
        let two = TWO_RESULTS.as_bytes();
        let out = rewrite(two, &other, "s", &generous(), panic_on_b, Some(&path));
        assert_eq!(out.unwrap().unwrap(), only_a_minified().into_bytes());
        assert!(receiver.recv(&mut buf).is_ok(), "encoded block reported");
        assert!(receiver.recv(&mut buf).is_err(), "unchanged block reported");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&store.dir);
        let _ = std::fs::remove_dir_all(&other.dir);
    }
}
