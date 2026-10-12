//! table-v2: a skeleton of the minified JSON with holes, followed by one
//! block per hole. The encoder replaces eligible array members of a top-level
//! object, so the wrapper's other members stay byte-identical. A block is a
//! table, or a bucket block that splits a heterogeneous array by a string
//! field. A `json` column holds string cells whose text is JSON as sub-blocks.
//! A bare top-level array keeps its table-v1 output unless a table-v2 form is
//! shorter.
//!
//! `docs/compression-formats.md` specifies the format. The round-trip contract
//! is `decode(&encode(x)?) == minified(x)`, byte for byte.

use std::fmt::Write;

use super::{TableOptions, bucket, decode_rows, encode_block, enough_gain, sub};
use crate::{MAX_JSON_DEPTH_CEILING, Reason, Scanner, minify};

/// First token of every table-v2 skeleton line.
pub const MARKER: &str = "table-v2";

/// Encodes `input` as table-v2, or as table-v1 when it is a bare array that
/// no table-v2 form shortens. `Err` means: pass the input through unchanged.
pub fn encode(input: &str, opts: &TableOptions) -> Result<String, Reason> {
    const NO: Reason = Reason::UnsupportedFormat;
    if input
        .trim_start_matches([' ', '\t', '\n', '\r'])
        .starts_with('[')
    {
        return encode_bare(input, opts);
    }
    let minified = String::from_utf8(minify(input, opts.max_depth)?).map_err(|_| NO)?;
    let body = encode_object(&minified, "", 0, opts).ok_or(NO)?;
    let out = format!("{MARKER} {body}");
    if !enough_gain(&minified, &out, opts) {
        return Err(Reason::InsufficientGain);
    }
    Ok(out)
}

/// The skeleton of the minified object `min` and its blocks, each block on
/// its own line after the label `sub::child(label, n)`. `level` is the
/// nesting level of the blocks. `None` when `min` is not an object or no
/// member array becomes a hole.
pub(super) fn encode_object(
    min: &str,
    label: &str,
    level: usize,
    opts: &TableOptions,
) -> Option<String> {
    let mut skeleton = String::new();
    let mut blocks = String::new();
    let mut copied = 0;
    let mut holes = 0;
    for span in array_members(min)? {
        let array = &min[span.clone()];
        let hole = format!("@{holes}");
        let child = sub::child(label, holes);
        let Some(block) = encode_array(array, &child, opts.max_rows) else {
            continue;
        };
        let block = sub::apply(&block, &child, level, opts);
        // The hole costs its text, its block label, a space and a line break.
        // Per-hole selection is this strict byte comparison only: no permille
        // or token threshold. The gain check runs once on the whole text.
        if hole.len() + child.len() + 2 + block.len() >= array.len() {
            continue;
        }
        skeleton.push_str(&min[copied..span.start]);
        skeleton.push_str(&hole);
        copied = span.end;
        let _ = write!(blocks, "\n{child} {block}");
        holes += 1;
    }
    if holes == 0 {
        return None;
    }
    skeleton.push_str(&min[copied..]);
    Some(skeleton + &blocks)
}

/// A bare array: table-v1 text, or `table-v2 @0` with one block when that is
/// strictly shorter. The block is the table-v1 table with sub-blocks, or a
/// bucket block, whichever is shorter. Rejections keep their table-v1 reason,
/// except that a block failing the gain check reports `InsufficientGain`.
fn encode_bare(input: &str, opts: &TableOptions) -> Result<String, Reason> {
    // ponytail: the input is minified twice, once inside `super::encode` and
    // once here. Each pass is linear, so the cost is one extra linear pass.
    // Share the minified text if profiling shows it matters.
    let v1 = super::encode(input, opts);
    let Some(minified) = minify(input, opts.max_depth)
        .ok()
        .and_then(|m| String::from_utf8(m).ok())
    else {
        return v1;
    };
    // The table block counts only when sub-blocks changed it.
    let table = encode_block(&minified, 2, opts.max_rows)
        .and_then(|block| Some(sub::apply(&block, "@0", 0, opts)).filter(|b| *b != block));
    let buckets =
        bucket::encode(&minified, "@0", opts.max_rows).map(|b| sub::apply(&b, "@0", 0, opts));
    let Some(block) = [table, buckets]
        .into_iter()
        .flatten()
        .min_by_key(String::len)
    else {
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
pub(super) fn encode_array(array: &str, label: &str, max_rows: usize) -> Option<String> {
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
    let mut i = 0;
    let out = decode_skeleton(rest, &mut i, "", 0)?;
    (i == rest.len()).then_some(out)
}

/// Decodes the skeleton line at `*i` of `text` and the blocks of its holes,
/// labelled `sub::child(label, n)` at nesting `level`, to minified JSON.
/// Moves `*i` to the end of the last block.
pub(super) fn decode_skeleton(
    text: &str,
    i: &mut usize,
    label: &str,
    level: usize,
) -> Option<String> {
    // Minified JSON holds no raw line break, so the skeleton is one line.
    let skeleton = &text[*i..*i + text[*i..].find('\n')?];
    *i += skeleton.len();
    let bytes = skeleton.as_bytes();
    let mut out = String::new();
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
        if skeleton[j - 1..end] != format!("@{holes}") {
            return None;
        }
        let child = sub::child(label, holes);
        let block_start = format!("\n{child} ");
        if !text[*i..].starts_with(&block_start) {
            return None;
        }
        *i += block_start.len();
        out.push_str(&skeleton[copied..j - 1]);
        out.push_str(&decode_array(text, i, &child, level)?);
        (copied, j, holes) = (end, end, holes + 1);
    }
    if holes == 0 {
        return None;
    }
    out.push_str(&skeleton[copied..]);
    // The substituted text must be valid minified JSON.
    let check = minify(&out, MAX_JSON_DEPTH_CEILING).ok()?;
    (check == out.as_bytes()).then_some(out)
}

/// Decodes the table or bucket block at `*i` of `text`, labelled `label` at
/// nesting `level`, to a minified JSON array.
pub(super) fn decode_array(text: &str, i: &mut usize, label: &str, level: usize) -> Option<String> {
    if bucket::is_bucket(&text[*i..]) {
        bucket::decode(text, i, label, level)
    } else {
        Some(format!(
            "[{}]",
            decode_rows(text, i, Some((label, level)))?.join(",")
        ))
    }
}
