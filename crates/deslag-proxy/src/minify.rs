//! Opt-in JSON minification of Bash tool results in Messages requests.
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
//! `content-encoding`, a body-integrity header or trailers pass through
//! unrecorded, so their new tool results stay undecided. A later request in
//! that session that resends them fails with `400`; start a new session.
//! Trailers are not forwarded upstream. `/v1/messages/count_tokens` is not
//! rewritten, so its counts can overstate.

use std::{
    collections::HashSet,
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use deslag_core::{FIRST_SLICE_JSON_MINIFY_CONTEXT, compress};
use http_body_util::BodyExt;
use hyper::{
    HeaderMap, Method, Request, StatusCode,
    body::{Body, Frame, Incoming, SizeHint},
    header,
    http::request::Parts,
};
use serde::de::{Deserialize, Deserializer, Error, MapAccess, Visitor};
use serde_json::value::RawValue;
use tokio::{sync::Semaphore, time::Duration};

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
/// body the permit is released and the rest streams without a deadline.
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
/// `POST /v1/messages` body is buffered and its tool results replayed.
/// Anything else streams through untouched. A failed buffered read is `400`.
/// A buffered read that exceeds `read_timeout` after its permit is granted is
/// `408`: the request is rejected, never forwarded unrewritten. The timeout
/// covers only the body read, never the replay. Replay errors are `400`, a
/// failed state commit is `500`; neither is forwarded.
pub(crate) async fn prepare(
    req: Request<Incoming>,
    store: Option<&Store>,
    read_timeout: Duration,
) -> Result<Request<ReqBody>, Reject> {
    let eligible = req.method() == Method::POST
        && req.uri().path() == "/v1/messages"
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
    let (mut parts, mut buf, trailers) =
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
    let scope = parts
        .headers
        .get(SCOPE_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    if let (Some(scope), None) = (scope, &trailers) {
        // Manifest IO blocks, so the replay runs off the async workers. The
        // blocking task owns the permit, so the buffer stays counted against
        // `INSPECT_PERMITS` until the replay ends, even if this future is dropped.
        let store = store.clone();
        let (back, result) = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let result = rewrite(&buf, &store, &scope);
            (buf, result)
        })
        .await
        .map_err(|_| STATE_FAILED)?;
        buf = back;
        if let Some(rewritten) = result.map_err(reject)? {
            buf = rewritten;
            parts
                .headers
                .insert(header::CONTENT_LENGTH, buf.len().into());
        }
    }
    let body = ReqBody {
        prefix: Some(buf.into()),
        rest: None,
        trailers,
    };
    Ok(Request::from_parts(parts, body))
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

/// Replays the session's decisions on the tool results and splices json-minify
/// output where they encode. `Ok(None)` forwards the body unchanged.
fn rewrite(body: &[u8], store: &Store, scope: &str) -> Result<Option<Vec<u8>>, ReplayError> {
    let Some((model, bash_ids, results)) = tool_results(body) else {
        return Ok(None);
    };
    let candidates: Vec<Candidate> = results
        .iter()
        .map(|(id, newest, targets)| Candidate {
            id,
            newest: *newest,
            texts: targets.iter().filter_map(|t| parse(t)).collect(),
        })
        .collect();
    // Only a Bash result whose text json-minify changes is encoded.
    let encode = store.replay(scope, model, &candidates, |c| {
        bash_ids.contains(c.id) && c.texts.iter().any(|t| minify(t).is_some())
    })?;
    let targets = results
        .iter()
        .zip(encode)
        .filter(|(_, encode)| *encode)
        .flat_map(|((_, _, targets), _)| targets.iter().copied());
    Ok(splice(body, targets))
}

fn minify(text: &str) -> Option<String> {
    compress(text, &FIRST_SLICE_JSON_MINIFY_CONTEXT).into_changed()
}

/// Model, Bash tool_use ids, and per tool result its id, whether it is in the
/// last message, and its string targets in document order.
type ToolResults<'a> = (
    &'a str,
    HashSet<&'a str>,
    Vec<(&'a str, bool, Vec<&'a RawValue>)>,
);

/// `None` when the body is not a JSON object with a non-empty `messages` array.
fn tool_results(body: &[u8]) -> Option<ToolResults<'_>> {
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
    let bash_ids = blocks("assistant")
        .filter(|(_, b)| b.str("type") == Some("tool_use") && b.str("name") == Some("Bash"))
        .filter_map(|(_, b)| b.str("id"))
        .collect();
    let mut results = Vec::new();
    for (i, block) in blocks("user") {
        let (Some("tool_result"), Some(id)) = (block.str("type"), block.str("tool_use_id")) else {
            continue;
        };
        let targets = match block.get("content") {
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
        results.push((id, i == last, targets));
    }
    Some((root.str("model").unwrap_or_default(), bash_ids, results))
}

/// Replaces each string target that json-minify changes. `None` when nothing changed.
fn splice<'a>(body: &[u8], targets: impl Iterator<Item = &'a RawValue>) -> Option<Vec<u8>> {
    // Targets are in document order, so spans are increasing and disjoint.
    let mut out = Vec::with_capacity(body.len());
    let mut pos = 0;
    for raw in targets {
        let Some(minified) = parse::<String>(raw).and_then(|s| minify(&s)) else {
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
    use http_body_util::BodyExt;
    use hyper::HeaderMap;

    use super::{ReqBody, rewrite};
    use crate::session::{Mode, Store};

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
            assert!(matches!(rewrite(body, &store, "s"), Ok(None)));
        }
    }
}
