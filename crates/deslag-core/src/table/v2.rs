//! table-v2: a skeleton of the minified JSON with holes, followed by one
//! block per hole. The encoder replaces eligible array members of a top-level
//! object, so the wrapper's other members stay byte-identical. A block is a
//! table, or a bucket block that splits a heterogeneous array by a string
//! field. A bare top-level array keeps its table-v1 output unless one bucket
//! block is shorter.
//!
//! `docs/compression-formats.md` specifies the format. The round-trip contract
//! is `decode(&encode(x)?) == minified(x)`, byte for byte.

use std::fmt::Write;

use super::{TableOptions, bucket, decode_block, encode_block, enough_gain};
use crate::{MAX_JSON_DEPTH_CEILING, Reason, Scanner, minify};

/// First token of every table-v2 skeleton line.
pub const MARKER: &str = "table-v2";

/// Encodes `input` as table-v2, or as table-v1 when it is a bare array that
/// bucketing does not shorten. `Err` means: pass the input through unchanged.
pub fn encode(input: &str, opts: &TableOptions) -> Result<String, Reason> {
    const NO: Reason = Reason::UnsupportedFormat;
    if input
        .trim_start_matches([' ', '\t', '\n', '\r'])
        .starts_with('[')
    {
        return encode_bare(input, opts);
    }
    let minified = String::from_utf8(minify(input, opts.max_depth)?).map_err(|_| NO)?;
    let mut skeleton = String::new();
    let mut blocks = String::new();
    let mut copied = 0;
    let mut holes = 0;
    for span in array_members(&minified).ok_or(NO)? {
        let array = &minified[span.clone()];
        let hole = format!("@{holes}");
        let Some(block) = encode_array(array, &hole, opts.max_rows) else {
            continue;
        };
        // The hole costs its label twice, a space and a line break. Per-hole
        // selection is this strict byte comparison only: no permille or token
        // threshold. The gain check runs once on the whole text below.
        if 2 * hole.len() + 2 + block.len() >= array.len() {
            continue;
        }
        skeleton.push_str(&minified[copied..span.start]);
        skeleton.push_str(&hole);
        copied = span.end;
        let _ = write!(blocks, "\n{hole} {block}");
        holes += 1;
    }
    if holes == 0 {
        return Err(NO);
    }
    skeleton.push_str(&minified[copied..]);
    let out = format!("{MARKER} {skeleton}{blocks}");
    if !enough_gain(&minified, &out, opts) {
        return Err(Reason::InsufficientGain);
    }
    Ok(out)
}

/// A bare array: table-v1 text, or `table-v2 @0` with one bucket block when
/// that is strictly shorter. Rejections keep their table-v1 reason, except
/// that a bucket block failing the gain check reports `InsufficientGain`.
fn encode_bare(input: &str, opts: &TableOptions) -> Result<String, Reason> {
    let v1 = super::encode(input, opts);
    let Some(minified) = minify(input, opts.max_depth)
        .ok()
        .and_then(|m| String::from_utf8(m).ok())
    else {
        return v1;
    };
    let Some(block) = bucket::encode(&minified, "@0", opts.max_rows) else {
        return v1;
    };
    let out = format!("{MARKER} @0\n@0 {block}");
    if v1.as_ref().is_ok_and(|t| t.len() <= out.len()) {
        return v1;
    }
    if !enough_gain(&minified, &out, opts) {
        return v1.or(Err(Reason::InsufficientGain));
    }
    Ok(out)
}

/// The shorter of the table block and the bucket block for one minified
/// array labelled `label`; the table block on a tie. `None` when neither
/// applies.
fn encode_array(array: &str, label: &str, max_rows: usize) -> Option<String> {
    [
        encode_block(array, 2, max_rows),
        bucket::encode(array, label, max_rows),
    ]
    .into_iter()
    .flatten()
    .min_by_key(String::len)
}

/// Byte spans of the member values of the minified top-level object that are
/// arrays. `None` when `min` is not an object.
fn array_members(min: &str) -> Option<Vec<std::ops::Range<usize>>> {
    let mut s = Scanner {
        b: min.as_bytes(),
        i: 0,
        out: Vec::new(),
        max_depth: MAX_JSON_DEPTH_CEILING,
    };
    s.expect(b'{').ok()?;
    let mut spans = Vec::new();
    if s.eat(b'}') {
        return Some(spans);
    }
    loop {
        s.string().ok()?;
        s.expect(b':').ok()?;
        let start = s.i;
        s.value(1).ok()?;
        if min.as_bytes()[start] == b'[' {
            spans.push(start..s.i);
        }
        if !s.eat(b',') {
            return Some(spans);
        }
    }
}

/// Decodes table-v2 or table-v1 text to minified JSON. `None` for malformed
/// text.
pub fn decode(text: &str) -> Option<String> {
    let Some(rest) = text.strip_prefix(MARKER) else {
        return super::decode(text);
    };
    let rest = rest.strip_prefix(' ')?;
    // Minified JSON holds no raw line break, so the skeleton is the first line.
    let (skeleton, _) = rest.split_once('\n')?;
    let bytes = skeleton.as_bytes();
    let mut out = String::new();
    let mut i = skeleton.len();
    let (mut copied, mut holes) = (0, 0);
    let (mut in_string, mut escaped) = (false, false);
    let mut j = 0;
    while j < bytes.len() {
        let c = bytes[j];
        j += 1;
        if in_string {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_string = false;
            }
            continue;
        }
        in_string = c == b'"';
        if c != b'@' {
            continue;
        }
        let end = skeleton[j..]
            .find(|c: char| !c.is_ascii_digit())
            .map_or(skeleton.len(), |e| j + e);
        // Holes are numbered 0, 1, 2, ... in skeleton order, and block `@n`
        // follows the previous block.
        let label = format!("@{holes}");
        if skeleton[j - 1..end] != label {
            return None;
        }
        let block_start = format!("\n{label} ");
        if !rest[i..].starts_with(&block_start) {
            return None;
        }
        i += block_start.len();
        out.push_str(&skeleton[copied..j - 1]);
        let block = if bucket::is_bucket(&rest[i..]) {
            bucket::decode(rest, &mut i, &label)?
        } else {
            decode_block(rest, &mut i)?
        };
        out.push_str(&block);
        (copied, j, holes) = (end, end, holes + 1);
    }
    if holes == 0 || i != rest.len() {
        return None;
    }
    out.push_str(&skeleton[copied..]);
    // The substituted text must be valid minified JSON.
    let check = minify(&out, MAX_JSON_DEPTH_CEILING).ok()?;
    (check == out.as_bytes()).then_some(out)
}
