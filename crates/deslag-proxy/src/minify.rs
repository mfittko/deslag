//! Opt-in JSON minification of Bash tool results in Messages requests.
//!
//! The request is never re-serialized. Each rewritten string lexeme is located
//! by its byte span in the original body and spliced; all other bytes are
//! forwarded unchanged. Every decision depends only on the tool_use block
//! (`id`, `name`) and the tool_result block, which the client resends
//! unchanged, so resending history yields the same bytes upstream.

use std::{
    collections::HashSet,
    fmt,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::{
    Method, Request,
    body::{Body, Frame, Incoming, SizeHint},
    header,
};
use serde::de::{Deserialize, Deserializer, Error, MapAccess, Visitor};
use serde_json::value::RawValue;

/// ponytail: at or above the Anthropic Messages request limit (32 MB), so every
/// request the provider accepts is inspected and the decision never depends on
/// size. A lower cap would bypass long sessions and change their history bytes.
const MAX_INSPECT_BYTES: usize = 32 * 1024 * 1024;

/// Request body: an optional buffered prefix followed by the rest of the original body.
pub(crate) struct ReqBody {
    prefix: Option<Bytes>,
    rest: Option<Incoming>,
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
            None => Poll::Ready(None),
        }
    }

    fn is_end_stream(&self) -> bool {
        self.prefix.as_ref().is_none_or(Bytes::is_empty)
            && self.rest.as_ref().is_none_or(Body::is_end_stream)
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
/// Anything else streams through untouched.
pub(crate) async fn prepare(
    req: Request<Incoming>,
    enabled: bool,
) -> Result<Request<ReqBody>, hyper::Error> {
    let eligible = enabled
        && req.method() == Method::POST
        && req.uri().path() == "/v1/messages"
        && !req.headers().contains_key(header::CONTENT_ENCODING);
    let (mut parts, mut body) = req.into_parts();
    if !eligible {
        let body = ReqBody {
            prefix: None,
            rest: Some(body),
        };
        return Ok(Request::from_parts(parts, body));
    }
    let mut buf = Vec::new();
    while let Some(frame) = body.frame().await {
        // ponytail: request trailers are dropped on the buffered path; Messages
        // clients do not send them. Forward them if a client ever does.
        let Ok(data) = frame?.into_data() else {
            continue;
        };
        buf.extend_from_slice(&data);
        if buf.len() > MAX_INSPECT_BYTES {
            // Never truncate or reject: forward the prefix, then the rest as it arrives.
            let body = ReqBody {
                prefix: Some(buf.into()),
                rest: Some(body),
            };
            return Ok(Request::from_parts(parts, body));
        }
    }
    if let Some(rewritten) = rewrite(&buf) {
        buf = rewritten;
        parts
            .headers
            .insert(header::CONTENT_LENGTH, buf.len().into());
    }
    let body = ReqBody {
        prefix: Some(buf.into()),
        rest: None,
    };
    Ok(Request::from_parts(parts, body))
}

/// Minifies the string content of tool_result blocks that answer a Bash
/// tool_use. Returns `None` when nothing changed or the body is not a JSON object.
fn rewrite(body: &[u8]) -> Option<Vec<u8>> {
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
        let Some(minified) = parse::<String>(raw).and_then(|s| deslag_core::minify_json(&s)) else {
            continue;
        };
        let start = raw.get().as_ptr() as usize - body.as_ptr() as usize;
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
    use super::rewrite;

    #[test]
    fn rewrite_skips_unchanged_and_invalid_bodies() {
        for body in [
            &b""[..],
            b"{",
            b"[]",
            br#"{"messages":[]}"#,
            br#"{"messages":[{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"[ 1 ]"}]}]}"#,
        ] {
            assert_eq!(rewrite(body), None);
        }
    }
}
