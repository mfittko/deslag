//! Sub-tables: a string cell whose text is minified JSON, an array of objects
//! or an object holding such arrays, becomes a reference cell `@k` in a `json`
//! column. Its sub-block follows the records of the table that holds it.
//!
//! `docs/compression-formats.md` (table-v2, Sub-tables) specifies the format.

use std::fmt::Write;

use super::{
    Cell, TableOptions, bucket, parse_count, parse_record, push_quoted, push_str_cell, v2,
};
use crate::{MAX_JSON_DEPTH_CEILING, minify};

/// Sub-blocks nest at most this many levels below a top-level block. Each
/// level escapes the decoded text once more: text of `m` bytes becomes at
/// most `2m + 2` bytes (doubled escapes plus two quotes). So nested content
/// of `n` bytes at level 0 decodes to at most `16n + 30` bytes.
pub(super) const MAX_NESTING: usize = 4;

/// The label of block `n` under `label`: `@n` at the top level, else `label.n`.
pub(super) fn child(label: &str, n: usize) -> String {
    if label.is_empty() {
        format!("@{n}")
    } else {
        format!("{label}.{n}")
    }
}

/// True for cell text with reference syntax: `@` and one or more digits.
pub(super) fn is_ref(cell: &str) -> bool {
    cell.strip_prefix('@')
        .is_some_and(|d| !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit()))
}

/// Rewrites the table or bucket `block` labelled `label` at nesting `level`
/// so that each string cell with a shorter sub-block becomes a reference.
/// Returns `block` unchanged when no cell qualifies or the rewrite is not
/// strictly shorter, for example when quoting `@n` look-alikes costs more.
pub(super) fn apply(block: &str, label: &str, level: usize, opts: &TableOptions) -> String {
    if level >= MAX_NESTING {
        return block.to_string();
    }
    // Encoder output always parses; `None` would only keep the block as is.
    rewrite(block, label, level, opts)
        .filter(|out| out.len() < block.len())
        .unwrap_or_else(|| block.to_string())
}

fn rewrite(block: &str, label: &str, level: usize, opts: &TableOptions) -> Option<String> {
    let mut out = String::new();
    let mut i = 0;
    if !bucket::is_bucket(block) {
        table(block, &mut i, label, level, opts, &mut out)?;
        return (i == block.len()).then_some(out);
    }
    // Copy the bucket header and order lines, then rewrite each bucket table.
    let header_end = block.find('\n')?;
    i = header_end + 1 + block[header_end + 1..].find('\n')?;
    out.push_str(&block[..i]);
    for k in 0.. {
        let table_label = child(label, k);
        let start = format!("\n{table_label} ");
        if !block[i..].starts_with(&start) {
            break;
        }
        out.push_str(&start);
        i += start.len();
        table(block, &mut i, &table_label, level, opts, &mut out)?;
    }
    (i == block.len()).then_some(out)
}

/// Rewrites the table block at `*i` of `text` into `out`, followed by the
/// sub-blocks of its reference cells. Cells are tried in record order, and
/// each becomes a reference only when that is strictly shorter.
fn table(
    text: &str,
    i: &mut usize,
    label: &str,
    level: usize,
    opts: &TableOptions,
    out: &mut String,
) -> Option<()> {
    let header_end = text[*i..].find('\n').map_or(text.len(), |e| *i + e);
    let (count, specs) = text[*i..header_end].strip_prefix('[')?.split_once("]{")?;
    let specs: Vec<(&str, &str)> = specs
        .strip_suffix('}')?
        .split(',')
        .map(|s| s.rsplit_once(':'))
        .collect::<Option<_>>()?;
    *i = header_end;
    let mut records = Vec::new();
    for _ in 0..parse_count(count)? {
        if !text[*i..].starts_with('\n') {
            return None;
        }
        *i += 1;
        records.push(parse_record(text, i)?);
    }

    let mut json = vec![false; specs.len()];
    let mut refs = vec![vec![false; specs.len()]; records.len()];
    let mut subs = String::new();
    let mut n = 0;
    for (r, cells) in records.iter().enumerate() {
        for (j, cell) in cells.iter().enumerate() {
            let text = match cell {
                Cell::Quoted(s) => s.as_str(),
                Cell::Bare(s) if !matches!(*s, "" | "null") => s,
                _ => continue,
            };
            if specs.get(j)?.1 != "str" {
                continue;
            }
            let sub_label = child(label, n);
            let Some(block) = encode(text, &sub_label, level + 1, opts) else {
                continue;
            };
            let mut old = String::new();
            push_str_cell(&mut old, text);
            let cost = format!("@{n}").len() + sub_label.len() + 2 + block.len();
            if cost >= old.len() {
                continue;
            }
            let _ = write!(subs, "\n{sub_label} {block}");
            (refs[r][j], json[j], n) = (true, true, n + 1);
        }
    }

    let _ = write!(out, "[{count}]{{");
    for (j, (path, ty)) in specs.iter().enumerate() {
        let sep = if j == 0 { "" } else { "," };
        let ty = if json[j] { "json" } else { ty };
        let _ = write!(out, "{sep}{path}:{ty}");
    }
    out.push('}');
    let mut n = 0;
    for (r, cells) in records.iter().enumerate() {
        out.push('\n');
        for (j, cell) in cells.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            match cell {
                _ if refs[r][j] => {
                    let _ = write!(out, "@{n}");
                    n += 1;
                }
                // A string with reference syntax is quoted in a `json` column.
                Cell::Bare(s) if json[j] && is_ref(s) => push_quoted(out, s),
                Cell::Quoted(s) if json[j] && is_ref(s) => push_quoted(out, s),
                Cell::Bare(s) => out.push_str(s),
                Cell::Quoted(s) => push_str_cell(out, s),
            }
        }
    }
    out.push_str(&subs);
    Some(())
}

/// The sub-block for the string text `text`, labelled `label` at nesting
/// `level`: a table or bucket block for an array, or a skeleton block
/// `= <skeleton>` with its own blocks for an object. `None` unless `text`
/// equals its own minified form within `max_depth`.
fn encode(text: &str, label: &str, level: usize, opts: &TableOptions) -> Option<String> {
    let first = *text.as_bytes().first()?;
    if !matches!(first, b'[' | b'{') || minify(text, opts.max_depth).ok()? != text.as_bytes() {
        return None;
    }
    if first == b'[' {
        let block = v2::encode_array(text, label, opts.max_rows)?;
        Some(apply(&block, label, level, opts))
    } else {
        Some(format!(
            "= {}",
            v2::encode_object(text, label, level, opts)?
        ))
    }
}

/// Decodes the sub-block at `*i` of `text`, labelled `label` at nesting
/// `level`, to minified JSON text. `None` for a malformed block or a level
/// beyond [`MAX_NESTING`].
pub(super) fn decode(text: &str, i: &mut usize, label: &str, level: usize) -> Option<String> {
    if level > MAX_NESTING {
        return None;
    }
    if text[*i..].starts_with("= ") {
        *i += 2;
        return v2::decode_skeleton(text, i, label, level);
    }
    let out = v2::decode_array(text, i, label, level)?;
    let check = minify(&out, MAX_JSON_DEPTH_CEILING).ok()?;
    (check == out.as_bytes()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_reference_lookalikes_stay_quoted_in_json_columns() {
        // The table-v1 encoder never quotes `@3`, so build the block by hand.
        let items: Vec<String> = (0..4)
            .map(|i| format!(r#"{{"k":{i},"v":"w{i}"}}"#))
            .collect();
        let mut block = String::from("[2]{a:str}\n\"@3\"\n");
        push_str_cell(&mut block, &format!("[{}]", items.join(",")));
        let out = apply(&block, "@0", 0, &TableOptions::default());
        assert!(out.starts_with("[2]{a:json}\n\"@3\"\n@0\n@0.0 "), "{out}");
    }
}
