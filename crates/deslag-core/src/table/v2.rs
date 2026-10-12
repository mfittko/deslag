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

use super::{TableOptions, bucket, decode_block, encode_block, enough_gain, sub};
use crate::{MAX_JSON_DEPTH_CEILING, Reason, Scanner, minify};

/// First token of every table-v2 skeleton line.
pub const MARKER: &str = "table-v2";

/// Encodes `input` as table-v2, or as table-v1 when it is a bare array that
/// no table-v2 form shortens. `Err` means: pass the input through unchanged.
pub fn encode(input: &str, opts: &TableOptions) -> Result<String, Reason> {
    encode_minified(input, opts).map(|(text, _)| text)
}

/// Like [`encode`], and also returns the minified input. The input is
/// minified once, so a caller that validates the text needs no second pass.
pub(crate) fn encode_minified(
    input: &str,
    opts: &TableOptions,
) -> Result<(String, String), Reason> {
    const NO: Reason = Reason::UnsupportedFormat;
    let minified = String::from_utf8(minify(input, opts.max_depth)?).map_err(|_| NO)?;
    if minified.starts_with('[') {
        let text = encode_bare(&minified, opts)?;
        return Ok((text, minified));
    }
    let body = encode_object(&minified, "", 0, opts).ok_or(NO)?;
    let out = format!("{MARKER} {body}");
    if !enough_gain(&minified, &out, opts) {
        return Err(Reason::InsufficientGain);
    }
    Ok((out, minified))
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
        let Some(block) = encode_array(array, &child, level, opts) else {
            continue;
        };
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

/// The minified bare array `min`: table-v1 text, or `table-v2 @0` with one
/// block when that is strictly shorter. A table block without sub-blocks is
/// never strictly shorter in bytes, because `table-v2 @0\n@0 ` is longer than
/// `table-v1 `. Selection compares bytes only, so a longer bucket block never
/// reaches the gain check, even when it would pass the token check. Rejections
/// keep their table-v1 reason, except that a block failing the gain check
/// reports `InsufficientGain`.
fn encode_bare(min: &str, opts: &TableOptions) -> Result<String, Reason> {
    // The table-v1 text is `table-v1 ` and the same table block that competes
    // below, so the block is built once.
    let table = encode_block(min, 2, opts.max_rows);
    let v1 = match &table {
        None => Err(Reason::UnsupportedFormat),
        Some(block) => Some(format!("{} {block}", super::MARKER))
            .filter(|t| enough_gain(min, t, opts))
            .ok_or(Reason::InsufficientGain),
    };
    let Some(block) = select(min, table, "@0", 0, opts) else {
        return v1;
    };
    let out = format!("{MARKER} @0\n@0 {block}");
    if v1.as_ref().is_ok_and(|t| t.len() <= out.len()) {
        return v1;
    }
    if !enough_gain(min, &out, opts) {
        return v1.or(Err(Reason::InsufficientGain));
    }
    Ok(out)
}

/// The block for one minified array labelled `label` at nesting `level`.
/// Sub-tables apply to the table block and to the bucket block, then the
/// shorter wins, the table block on a tie. `None` when neither applies.
pub(super) fn encode_array(
    array: &str,
    label: &str,
    level: usize,
    opts: &TableOptions,
) -> Option<String> {
    select(
        array,
        encode_block(array, 2, opts.max_rows),
        label,
        level,
        opts,
    )
}

/// [`encode_array`] with the table block of `array` already built.
// ponytail: both candidates recurse, so text at nesting level `k` is encoded
// up to 2^k times, at most 2^MAX_NESTING. Still linear. A sub-block embeds
// its label, and the two candidates label the same cell differently, so a
// cache would have to relabel blocks. Add one if nested inputs show it matters.
fn select(
    array: &str,
    table: Option<String>,
    label: &str,
    level: usize,
    opts: &TableOptions,
) -> Option<String> {
    // A cell becomes a sub-block only when its string starts with `[` or `{`.
    // Strings hold only canonical escapes, so such a string starts `"[` or
    // `"{` in the minified array. Without one, sub::apply returns each block
    // unchanged, and its parse and rewrite are skipped.
    let nested = array.contains("\"[") || array.contains("\"{");
    [table, bucket::encode(array, label, opts.max_rows)]
        .into_iter()
        .flatten()
        .map(|block| {
            if nested {
                sub::apply(&block, label, level, opts)
            } else {
                block
            }
        })
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
        decode_block(text, i, Some((label, level)))
    }
}
