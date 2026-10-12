//! Opt-in JSON minification of Bash tool results in Messages requests.
//!
//! The request is never re-serialized. Each rewritten string lexeme is located
//! by its byte span in the original body and spliced; all other bytes are
//! forwarded unchanged. Every decision depends only on the tool_use block
//! (`id`, `name`) and the tool_result block, which the client resends
//! unchanged, so resending history yields the same bytes upstream.
//!
//! Experimental. The rewrite is stateless and keeps no replay manifest yet.
//! Enabling, disabling or upgrading `--minify-json` mid-session rewrites
//! already-submitted history on resend. Start a new session after any such
//! change. Do not enable the hook and proxy flags at the same time: one
//! component owns compression. Requests with `content-encoding` pass through,
//! so a client that compresses only some requests would see drift. Requests
//! with a body-integrity header or with trailers also pass through. Trailers
//! are not forwarded upstream. `/v1/messages/count_tokens` is not rewritten,
//! so its counts can overstate. Each tool result is compressed as one job of
//! `Limits::executor`. When the executor rejects any job (busy, budget exceeded
//! or panicked), the whole body is forwarded unrewritten, so under load a resend can differ from the
//! first send.

use std::{
    collections::HashSet,
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use deslag_core::{BoundedExecutor, FIRST_SLICE_JSON_MINIFY_CONTEXT, compress};
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
/// The deadline covers only the buffered part, up to `MAX_INSPECT_BYTES`. For a
/// larger body the permit is released and the rest streams without a deadline.
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

/// Returns the request to forward. With `enabled`, an uncompressed
/// `POST /v1/messages` body is buffered and its Bash tool results minified.
/// Anything else streams through untouched. A failed buffered read is `400`.
/// A buffered read that exceeds `read_timeout` after its permit is granted is
/// `408`: the request is rejected, never forwarded unrewritten.
pub(crate) async fn prepare(
    req: Request<Incoming>,
    enabled: bool,
    read_timeout: Duration,
    exec: BoundedExecutor,
) -> Result<Request<ReqBody>, StatusCode> {
    let eligible = enabled
        && req.method() == Method::POST
        && req.uri().path() == "/v1/messages"
        && !req.headers().contains_key(header::CONTENT_ENCODING)
        && !INTEGRITY_HEADERS
            .iter()
            .any(|h| req.headers().contains_key(*h));
    let (parts, body) = req.into_parts();
    if !eligible {
        let body = ReqBody {
            prefix: None,
            rest: Some(body),
            trailers: None,
        };
        return Ok(Request::from_parts(parts, body));
    }
    let _permit = INSPECT_PERMITS
        .acquire()
        .await
        .expect("semaphore is never closed");
    // The deadline starts at the permit, so time spent queued does not count.
    match tokio::time::timeout(read_timeout, buffer(parts, body, exec)).await {
        Ok(Ok(req)) => Ok(req),
        Ok(Err(_)) => Err(StatusCode::BAD_REQUEST),
        Err(_) => Err(StatusCode::REQUEST_TIMEOUT),
    }
}

/// Buffers up to `MAX_INSPECT_BYTES` of `body` and rewrites it once complete.
/// A body with trailers is never rewritten, because a trailer such as
/// `content-digest` can pin its bytes. The trailers are kept in the body, but
/// they do not reach the upstream: the hop-by-hop strip removes the `trailer`
/// header, and a `content-length` body carries no trailers over HTTP/1.1.
async fn buffer(
    mut parts: Parts,
    mut body: Incoming,
    exec: BoundedExecutor,
) -> Result<Request<ReqBody>, hyper::Error> {
    let hint = body.size_hint().exact().unwrap_or(0);
    let mut buf = Vec::with_capacity(hint.min(MAX_INSPECT_BYTES as u64 + 1) as usize);
    let mut trailers: Option<HeaderMap> = None;
    while let Some(frame) = body.frame().await {
        let data = match frame?.into_data() {
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
            return Ok(Request::from_parts(parts, body));
        }
    }
    if trailers.is_some() {
        let body = ReqBody {
            prefix: Some(buf.into()),
            rest: None,
            trailers,
        };
        return Ok(Request::from_parts(parts, body));
    }
    // The rewrite blocks, so it runs on the blocking pool. A panic there forwards the original.
    let mut buf = Bytes::from(buf);
    let input = buf.clone();
    let rewritten = tokio::task::spawn_blocking(move || rewrite(&input, &exec, minify)).await;
    if let Ok(Some(rewritten)) = rewritten {
        buf = rewritten.into();
        parts
            .headers
            .insert(header::CONTENT_LENGTH, buf.len().into());
    }
    let body = ReqBody {
        prefix: Some(buf),
        rest: None,
        trailers: None,
    };
    Ok(Request::from_parts(parts, body))
}

/// Per-text compression job. Tests inject a slow one.
type Job = fn(&str) -> Option<String>;

fn minify(text: &str) -> Option<String> {
    compress(text, &FIRST_SLICE_JSON_MINIFY_CONTEXT).into_changed()
}

/// Minifies the string content of tool_result blocks that answer a Bash
/// tool_use. Each text runs as its own `job` under `exec`. Returns `None` when
/// nothing changed, the body is not a JSON object or any job is rejected.
fn rewrite(body: &[u8], exec: &BoundedExecutor, job: Job) -> Option<Vec<u8>> {
    let root: Obj = serde_json::from_slice(body).ok()?;
    let messages: Vec<Obj> = parse::<Vec<&RawValue>>(root.get("messages")?)?
        .into_iter()
        .filter_map(parse)
        .collect();
    let blocks = |role: &'static str| {
        messages
            .iter()
            .filter(move |m| m.str("role") == Some(role))
            .filter_map(|m| parse::<Vec<&RawValue>>(m.get("content")?))
            .flatten()
            .filter_map(parse::<Obj>)
    };
    let bash_ids: HashSet<&str> = blocks("assistant")
        .filter(|b| b.str("type") == Some("tool_use") && b.str("name") == Some("Bash"))
        .filter_map(|b| b.str("id"))
        .collect();
    let mut targets = Vec::new();
    for block in blocks("user") {
        if block.str("type") != Some("tool_result")
            || !block
                .str("tool_use_id")
                .is_some_and(|id| bash_ids.contains(id))
        {
            continue;
        }
        let Some(content) = block.get("content") else {
            continue;
        };
        match parse::<Vec<&RawValue>>(content) {
            Some(items) => targets.extend(
                items
                    .into_iter()
                    .filter_map(parse::<Obj>)
                    .filter(|item| item.str("type") == Some("text"))
                    .filter_map(|item| item.get("text")),
            ),
            None => targets.push(content),
        }
    }
    // Targets are in document order, so spans are increasing and disjoint.
    let mut out = Vec::with_capacity(body.len());
    let mut pos = 0;
    for raw in targets {
        let Some(text) = parse::<String>(raw) else {
            continue;
        };
        let Some(minified) = exec.run(move || job(&text)).ok()? else {
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
    use std::time::Duration;

    use deslag_core::BoundedExecutor;
    use http_body_util::BodyExt;
    use hyper::HeaderMap;

    use super::{ReqBody, minify, rewrite};

    /// A budget no debug build on a slow runner can miss.
    fn generous() -> BoundedExecutor {
        BoundedExecutor::new(4, Duration::from_secs(60)).unwrap()
    }

    /// Minifies, but far slower than any budget the tests below use for it.
    fn slow_on_b(text: &str) -> Option<String> {
        if text.contains("\"b\"") {
            std::thread::sleep(Duration::from_secs(10));
        }
        minify(text)
    }

    /// Two Bash tool results; the second one holds key `b`.
    const TWO_RESULTS: &str = r#"{"messages":[
{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}},{"type":"tool_use","id":"t2","name":"Bash","input":{}}]},
{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"{\n  \"a\": 1\n}"},{"type":"tool_result","tool_use_id":"t2","content":"{\n  \"b\": 1\n}"}]}]}"#;

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
            assert_eq!(rewrite(body, &generous(), minify), None);
        }
    }

    #[test]
    fn rewrite_minifies_every_target() {
        let expected = TWO_RESULTS
            .replace(r#""{\n  \"a\": 1\n}""#, r#""{\"a\":1}""#)
            .replace(r#""{\n  \"b\": 1\n}""#, r#""{\"b\":1}""#);
        let out = rewrite(TWO_RESULTS.as_bytes(), &generous(), minify).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), expected);
    }

    #[test]
    fn one_expired_job_forwards_the_original_body() {
        let exec = BoundedExecutor::new(1, Duration::from_secs(2)).unwrap();
        assert_eq!(rewrite(TWO_RESULTS.as_bytes(), &exec, slow_on_b), None);
    }

    #[test]
    fn busy_executor_forwards_the_original_body() {
        let exec = BoundedExecutor::new(1, Duration::from_secs(60)).unwrap();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let holder = {
            let exec = exec.clone();
            std::thread::spawn(move || exec.run(move || wait.recv()))
        };
        while exec.in_flight() == 0 {
            std::thread::yield_now();
        }
        assert_eq!(rewrite(TWO_RESULTS.as_bytes(), &exec, minify), None);
        drop(release);
        assert!(holder.join().unwrap().is_ok());
        assert!(rewrite(TWO_RESULTS.as_bytes(), &exec, minify).is_some());
    }
}
