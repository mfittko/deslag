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
//! so its counts can overstate. Each text of a Bash tool result is compressed
//! as one job of `Limits::executor`: each text item of a content array, or the
//! string content. When the executor rejects any job (busy, budget exceeded or
//! panicked), the whole body is forwarded unrewritten, so under load a resend
//! can differ from the first send.

use std::{
    collections::HashSet,
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
use serde_json::value::RawValue;
use tokio::{
    sync::{Semaphore, SemaphorePermit},
    time::Duration,
};

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
/// After the read, the permit stays held through the rewrite. The body parse
/// and rebuild have no time limit. The jobs run in sequence, and the budget of
/// `Limits::executor` bounds each job only.
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
/// `408`: the request is rejected, never forwarded unrewritten. The rewrite
/// runs after the read, outside that deadline, and holds the permit until it
/// ends, also when the client disconnects mid-rewrite. A rejected or panicked
/// rewrite forwards the original body.
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
    let permit = INSPECT_PERMITS
        .acquire()
        .await
        .expect("semaphore is never closed");
    // The deadline starts at the permit, so time spent queued does not count.
    let (mut parts, buf) = match tokio::time::timeout(read_timeout, buffer(parts, body)).await {
        Ok(Ok(Buffered::Complete(parts, buf))) => (parts, buf),
        Ok(Ok(Buffered::Forward(req))) => return Ok(req),
        Ok(Err(_)) => return Err(StatusCode::BAD_REQUEST),
        Err(_) => return Err(StatusCode::REQUEST_TIMEOUT),
    };
    let mut buf = Bytes::from(buf);
    if let Some(rewritten) = rewrite_blocking(permit, buf.clone(), exec, minify).await {
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

/// Runs [`rewrite`] on the blocking pool, outside the read deadline. The permit
/// moves into the blocking task, so it is released only when the rewrite ends,
/// even when the caller is dropped (client disconnect). A rejected job or a
/// panic returns `None`, so the original body is forwarded. The telemetry
/// socket lookup (one `stat`) and the report sends run here too.
async fn rewrite_blocking(
    permit: SemaphorePermit<'static>,
    input: Bytes,
    exec: BoundedExecutor,
    job: Job,
) -> Option<Vec<u8>> {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        rewrite(&input, &exec, job, local::live_socket().as_deref())
    })
    .await
    .ok()
    .flatten()
}

/// Result of [`buffer`]: a complete body to rewrite, or a request to forward as is.
enum Buffered {
    Complete(Parts, Vec<u8>),
    Forward(Request<ReqBody>),
}

/// Buffers up to `MAX_INSPECT_BYTES` of `body`. A body with trailers is never
/// rewritten, because a trailer such as `content-digest` can pin its bytes. The
/// trailers are kept in the body, but they do not reach the upstream: the
/// hop-by-hop strip removes the `trailer` header, and a `content-length` body
/// carries no trailers over HTTP/1.1.
async fn buffer(parts: Parts, mut body: Incoming) -> Result<Buffered, hyper::Error> {
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
            return Ok(Buffered::Forward(Request::from_parts(parts, body)));
        }
    }
    if trailers.is_some() {
        let body = ReqBody {
            prefix: Some(buf.into()),
            rest: None,
            trailers,
        };
        return Ok(Buffered::Forward(Request::from_parts(parts, body)));
    }
    Ok(Buffered::Complete(parts, buf))
}

/// One finished job: the changed text, if any, and the compression report.
type Compressed = (Option<String>, CompressionReport);

/// Per-text compression job. Tests inject a slow one.
type Job = fn(&str) -> Compressed;

fn minify(text: &str) -> Compressed {
    let result = compress(text, &FIRST_SLICE_JSON_MINIFY_CONTEXT);
    let report = result.report.clone();
    (result.into_changed(), report)
}

/// Minifies the string content of tool_result blocks that answer a Bash
/// tool_use. Each text runs as its own `job` under `exec`. Returns `None` when
/// nothing changed, the body is not a JSON object or any job is rejected.
/// With `socket`, every compressed block is reported there once all jobs have
/// returned; a body with a rejected job reports nothing. History resends
/// repeat blocks, so the proxy counts submissions, not unique results.
fn rewrite(
    body: &[u8],
    exec: &BoundedExecutor,
    job: Job,
    socket: Option<&Path>,
) -> Option<Vec<u8>> {
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
    let mut reports = Vec::new();
    for raw in targets {
        let Some(text) = parse::<String>(raw) else {
            continue;
        };
        let (text, (changed, mut compressed)) = exec
            .run(move || {
                let done = job(&text);
                (text, done)
            })
            .ok()?;
        if socket.is_some() {
            local::add_estimates(&mut compressed, &text, changed.as_deref().unwrap_or(&text));
            reports.push(compressed);
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
        for compressed in &reports {
            local::send_report(
                socket,
                compressed,
                local::Adapter::Proxy,
                local::ToolCategory::Bash,
            );
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
            assert_eq!(rewrite(body, &generous(), minify, None), None);
        }
    }

    #[test]
    fn rewrite_minifies_every_target() {
        let expected = TWO_RESULTS
            .replace(r#""{\n  \"a\": 1\n}""#, r#""{\"a\":1}""#)
            .replace(r#""{\n  \"b\": 1\n}""#, r#""{\"b\":1}""#);
        let out = rewrite(TWO_RESULTS.as_bytes(), &generous(), minify, None).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), expected);
    }

    #[test]
    fn one_expired_job_forwards_the_original_body() {
        let exec = BoundedExecutor::new(1, Duration::from_millis(200)).unwrap();
        assert_eq!(
            rewrite(TWO_RESULTS.as_bytes(), &exec, slow_on_b, None),
            None
        );
    }

    #[test]
    fn panicked_job_forwards_the_original_body() {
        fn panic_on_b(text: &str) -> super::Compressed {
            assert!(!text.contains("\"b\""), "injected panic");
            minify(text)
        }
        assert_eq!(
            rewrite(TWO_RESULTS.as_bytes(), &generous(), panic_on_b, None),
            None
        );
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
        let permit = PERMITS.acquire().await.unwrap();
        let input = bytes::Bytes::from_static(TWO_RESULTS.as_bytes());
        // Polls the rewrite until it is spawned, then drops it like a disconnect.
        let fut = rewrite_blocking(permit, input, generous(), gated);
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
        assert_eq!(rewrite(TWO_RESULTS.as_bytes(), &exec, minify, None), None);
        drop(release);
        assert!(holder.join().unwrap().is_ok());
        assert!(rewrite(TWO_RESULTS.as_bytes(), &exec, minify, None).is_some());
    }

    #[test]
    fn rewrite_output_does_not_depend_on_the_service() {
        use std::os::unix::net::UnixDatagram;
        let body = br#"{"messages":[{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash"}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"[ 1 ]"}]}]}"#;
        let plain = rewrite(body, &generous(), minify, None).expect("minified");
        let dir = std::path::PathBuf::from(format!("/tmp/dsl-proxy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.sock");
        // A stale socket file: the service is gone.
        drop(UnixDatagram::bind(&path).unwrap());
        assert_eq!(
            rewrite(body, &generous(), minify, Some(&path)).as_ref(),
            Some(&plain)
        );
        std::fs::remove_file(&path).unwrap();
        let receiver = UnixDatagram::bind(&path).unwrap();
        receiver.set_nonblocking(true).unwrap();
        assert_eq!(
            rewrite(body, &generous(), minify, Some(&path)).as_ref(),
            Some(&plain)
        );
        let mut buf = [0; 1024];
        let n = receiver.recv(&mut buf).unwrap();
        let report: serde_json::Value = serde_json::from_slice(&buf[..n]).unwrap();
        assert_eq!(report["adapter"], "proxy");
        assert_eq!(report["outcome"], "changed");
        // A body with a panicked job reports none of its blocks.
        fn panic_on_b(text: &str) -> super::Compressed {
            assert!(!text.contains("\"b\""), "injected panic");
            minify(text)
        }
        let two = TWO_RESULTS.as_bytes();
        assert_eq!(rewrite(two, &generous(), panic_on_b, Some(&path)), None);
        assert!(receiver.recv(&mut buf).is_err(), "rejected body reported");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
