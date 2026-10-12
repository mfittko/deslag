//! Discriminator buckets: a heterogeneous array of objects becomes one table
//! block per value of a string field, the discriminator, plus an order line
//! that restores the original row order.
//!
//! `docs/compression-formats.md` (table-v2, Bucket blocks) specifies the block.

use std::{cmp::Ordering, collections::HashMap, fmt::Write, ops::Range};

use super::{decode_rows, encode_block, parse_count, valid_name};
use crate::{MAX_JSON_DEPTH_CEILING, Scanner};

/// Discriminator candidates tried per array, in first-row key order.
// ponytail: each candidate re-encodes the whole array, so the cap keeps the
// encoder linear. Raise it if real discriminators sit past the eighth key.
const MAX_CANDIDATES: usize = 8;

/// One array item: its span and its members as (key, value span).
type Item<'a> = (Range<usize>, Vec<(&'a str, Range<usize>)>);

/// The shortest bucket block for the minified `array`, with bucket labels
/// `<label>.<k>`. `None` when no candidate yields two or more buckets that
/// each pass the table-v1 eligibility rules.
pub(super) fn encode(array: &str, label: &str, max_rows: usize) -> Option<String> {
    let items = items(array)?;
    if items.len() < 2 || items.len() > max_rows {
        return None;
    }
    // Number of items in which each key holds a string.
    let mut strings: HashMap<&str, usize> = HashMap::new();
    for (key, value) in items.iter().flat_map(|(_, members)| members) {
        if array.as_bytes()[value.start] == b'"' {
            *strings.entry(key).or_default() += 1;
        }
    }
    items[0]
        .1
        .iter()
        .map(|&(key, _)| key)
        .filter(|key| !key.is_empty() && valid_name(key) && strings.get(key) == Some(&items.len()))
        .take(MAX_CANDIDATES)
        .filter_map(|key| encode_by(array, &items, key, label))
        .min_by_key(String::len)
}

/// The bucket block that splits `items` by the string value of `key`.
fn encode_by(array: &str, items: &[Item], key: &str, label: &str) -> Option<String> {
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut buckets: Vec<Vec<&str>> = Vec::new();
    let mut order = Vec::with_capacity(items.len());
    for (span, members) in items {
        let (_, value) = members.iter().find(|(k, _)| *k == key)?;
        // Buckets are numbered in order of their first row.
        let b = *index.entry(&array[value.clone()]).or_insert_with(|| {
            buckets.push(Vec::new());
            buckets.len() - 1
        });
        buckets[b].push(&array[span.clone()]);
        order.push(b.to_string());
    }
    // ponytail: a key unique in every row gives one-row tables that each
    // repeat the header, so it is skipped before the tables are encoded.
    // `buckets.len() * 2 > items.len()` would skip more but drops winners
    // such as the spec example (3 buckets over 5 rows).
    if buckets.len() < 2 || buckets.len() == items.len() {
        return None;
    }
    let mut out = format!("[{}] by {key}\n{}", items.len(), order.join(","));
    for (k, rows) in buckets.iter().enumerate() {
        let block = encode_block(&format!("[{}]", rows.join(",")), 1, usize::MAX)?;
        let _ = write!(out, "\n{label}.{k} {block}");
    }
    Some(out)
}

/// The items of a minified array of objects. `None` when `min` is not a
/// non-empty array of objects.
fn items(min: &str) -> Option<Vec<Item<'_>>> {
    let mut s = Scanner {
        b: min.as_bytes(),
        i: 0,
        out: Vec::new(),
        max_depth: MAX_JSON_DEPTH_CEILING,
    };
    s.expect(b'[').ok()?;
    let mut items = Vec::new();
    loop {
        let start = s.i;
        s.expect(b'{').ok()?;
        let mut members = Vec::new();
        if !s.eat(b'}') {
            loop {
                let k = s.i;
                s.string().ok()?;
                let key = &min[k + 1..s.i - 1];
                s.expect(b':').ok()?;
                let v = s.i;
                s.value(1).ok()?;
                members.push((key, v..s.i));
                if !s.eat(b',') {
                    break;
                }
            }
            s.expect(b'}').ok()?;
        }
        items.push((start..s.i, members));
        if !s.eat(b',') {
            break;
        }
    }
    s.expect(b']').ok()?;
    Some(items)
}

/// True when the block at the start of `block` has a bucket header.
pub(super) fn is_bucket(block: &str) -> bool {
    let header = block.split('\n').next().unwrap_or_default();
    header
        .split_once(']')
        .is_some_and(|(_, rest)| rest.starts_with(" by "))
}

/// Decodes the bucket block at `*i` of `text` to a minified JSON array and
/// moves `*i` to the end of its last record. `label` is the block's own
/// label. `None` for a malformed block.
pub(super) fn decode(text: &str, i: &mut usize, label: &str) -> Option<String> {
    let line_end = |at: usize| text[at..].find('\n').map_or(text.len(), |e| at + e);
    let header_end = line_end(*i);
    let (count, key) = text[*i..header_end]
        .strip_prefix('[')?
        .split_once("] by ")?;
    let count = parse_count(count)?;
    if count == 0 || key.is_empty() || !valid_name(key) || !text[header_end..].starts_with('\n') {
        return None;
    }
    let order_end = line_end(header_end + 1);
    let mut order = Vec::new();
    let mut sizes: Vec<usize> = Vec::new();
    for index in text[header_end + 1..order_end].split(',') {
        let b = parse_count(index)?;
        // Buckets are numbered in order of their first row, so none is empty.
        match b.cmp(&sizes.len()) {
            Ordering::Less => sizes[b] += 1,
            Ordering::Equal => sizes.push(1),
            Ordering::Greater => return None,
        }
        order.push(b);
    }
    if order.len() != count {
        return None;
    }
    *i = order_end;
    let mut buckets = Vec::with_capacity(sizes.len());
    for (k, &size) in sizes.iter().enumerate() {
        let start = format!("\n{label}.{k} ");
        if !text[*i..].starts_with(&start) {
            return None;
        }
        *i += start.len();
        // The discriminator is a column of every bucket.
        let specs = text[*i..line_end(*i)]
            .split_once("]{")?
            .1
            .strip_suffix('}')?;
        if !specs
            .split(',')
            .any(|c| c.rsplit_once(':').is_some_and(|(path, _)| path == key))
        {
            return None;
        }
        let rows = decode_rows(text, i)?;
        if rows.len() != size {
            return None;
        }
        buckets.push(rows.into_iter());
    }
    let rows: Vec<String> = order
        .iter()
        .map(|&b| buckets[b].next())
        .collect::<Option<_>>()?;
    Some(format!("[{}]", rows.join(",")))
}
