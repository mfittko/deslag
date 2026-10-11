//! `lines-v2`: `lines-v1` plus template directives for runs of consecutive lines
//! that differ only in variable tokens.
//!
//! The format is specified in `docs/compression-formats.md`.

use crate::lines::{
    DecodeError, ESCAPE, HEADER, PREFIX, push_bounded, push_unit, run_directive, unit_cost,
};

/// Stable transform identity for the line-template codec.
pub const LINE_TEMPLATES_ID: &str = "line-templates";
/// Bumped whenever [`encode_templates`] output can change for some input.
pub const LINE_TEMPLATES_VERSION: u32 = 1;
/// Version of the `lines-v2` container.
pub const LINES_V2_FORMAT_VERSION: u32 = 2;

pub(crate) const HEADER_V2: &str = "#!deslag lines-v2\n";
pub(crate) const TEMPLATE: &str = "#!deslag t";

/// Encodes `input` as `lines-v2`, with no overall gain check. `None` when `input`
/// already starts with the `lines-v1` or `lines-v2` header.
pub fn encode_templates(input: &str) -> Option<String> {
    if input.starts_with(HEADER) || input.starts_with(HEADER_V2) {
        return None;
    }
    let units: Vec<&str> = input.split_inclusive('\n').collect();
    let mut out = String::from(HEADER_V2);
    let mut i = 0;
    // ponytail: when a group does not pay, one retry starts at the next unit, so
    // an outlier base does not block the lines below it. When the retry also
    // fails, no group starts inside either span, so each unit is scanned a
    // bounded number of times. A shorter group in that span could still pay; it
    // is left literal.
    let mut no_group_before = 0;
    let mut retry_at = None;
    while i < units.len() {
        let unit = units[i];
        let count = units[i..].iter().take_while(|&&u| u == unit).count();
        if count >= 2
            && let Some(directive) = run_directive(unit, count)
        {
            push_unit(&mut out, unit);
            out.push_str(&directive);
            i += count;
            continue;
        }
        if i >= no_group_before || retry_at == Some(i) {
            let (len, group) = template_group(&units[i..]);
            if let Some(group) = group {
                out.push_str(&group);
                retry_at = None;
                i += len;
                continue;
            }
            if retry_at.take() == Some(i) {
                no_group_before = no_group_before.max(i + len);
            } else {
                no_group_before = i + len;
                retry_at = Some(i + 1);
            }
        }
        push_unit(&mut out, unit);
        i += 1;
    }
    Some(out)
}

/// Grows a group greedily from `units[0]`. Returns the group length and its
/// encoding, which is `None` when the group does not pay.
fn template_group(units: &[&str]) -> (usize, Option<String>) {
    let Some(base) = units[0].strip_suffix('\n') else {
        return (1, None);
    };
    let base = split(base);
    let rows: Vec<Vec<&str>> = units[1..]
        .iter()
        .map_while(|u| u.strip_suffix('\n').map(split))
        .take_while(|row| fits(&base, row))
        .collect();
    let len = rows.len() + 1;
    let positions: Vec<usize> = (1..base.len())
        .step_by(2)
        .filter(|&k| rows.iter().any(|row| row[k] != base[k]))
        .collect();
    if positions.is_empty() {
        return (len, None);
    }
    let list: Vec<String> = positions.iter().map(|k| (k / 2).to_string()).collect();
    let mut body = format!("{TEMPLATE}{len} {}\n", list.join(","));
    for row in &rows {
        let values: Vec<&str> = positions.iter().map(|&k| row[k]).collect();
        push_unit(&mut body, &(values.join(" ") + "\n"));
    }
    let literal: usize = units[1..len].iter().map(|u| unit_cost(u)).sum();
    if body.len() >= literal {
        return (len, None);
    }
    let mut out = String::new();
    push_unit(&mut out, units[0]);
    out.push_str(&body);
    (len, Some(out))
}

/// Splits line content into alternating gaps and tokens:
/// `gap0 tok0 gap1 ... tok(n-1) gapn`. Tokens sit at odd indices.
fn split(content: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut in_gap) = (0, true);
    for (i, b) in content.bytes().enumerate() {
        let gap = b == b' ' || b == b'\t';
        if gap != in_gap {
            parts.push(&content[start..i]);
            (start, in_gap) = (i, gap);
        }
    }
    parts.push(&content[start..]);
    if !in_gap {
        parts.push("");
    }
    parts
}

/// Same gaps, same token count, and every token equal or both variable-class.
fn fits(base: &[&str], row: &[&str]) -> bool {
    base.len() == row.len()
        && base
            .iter()
            .zip(row)
            .enumerate()
            .all(|(k, (a, b))| a == b || (k % 2 == 1 && is_variable(a) && is_variable(b)))
}

fn is_variable(token: &str) -> bool {
    token
        .bytes()
        .any(|b| b.is_ascii_digit() || b == b'/' || b == b'\\')
}

/// Decodes the rows of the template directive `line` against `base`, the unit
/// directly above it, and appends the group's lines after `base`.
pub(crate) fn decode_group<'a>(
    line: &str,
    base: &'a str,
    rows: &mut impl Iterator<Item = &'a str>,
    out: &mut String,
    max: usize,
) -> Result<(), DecodeError> {
    let bad = DecodeError::InvalidDirective;
    let (count, list) = line
        .strip_prefix(TEMPLATE)
        .and_then(|d| d.strip_suffix('\n'))
        .and_then(|d| d.split_once(' '))
        .ok_or(bad)?;
    let count = canonical(count).filter(|&n| n >= 2).ok_or(bad)?;
    let positions = list
        .split(',')
        .map(|p| canonical(p).and_then(|k| k.checked_mul(2)).map(|k| k + 1))
        .collect::<Option<Vec<usize>>>()
        .ok_or(bad)?;
    let mut parts = split(base.strip_suffix('\n').ok_or(bad)?);
    if positions.windows(2).any(|w| w[0] >= w[1]) || positions.last() >= Some(&parts.len()) {
        return Err(bad);
    }
    for _ in 1..count {
        let row = rows.next().ok_or(bad)?;
        let row = match row.strip_prefix(ESCAPE) {
            Some(r) if r.starts_with(PREFIX) => r,
            None if !row.starts_with(PREFIX) => row,
            _ => return Err(bad),
        };
        let values: Vec<&str> = row.strip_suffix('\n').ok_or(bad)?.split(' ').collect();
        if values.len() != positions.len()
            || values.iter().any(|v| v.is_empty() || v.contains('\t'))
        {
            return Err(bad);
        }
        for (&k, value) in positions.iter().zip(values) {
            parts[k] = value;
        }
        push_bounded(out, &(parts.concat() + "\n"), 1, max)?;
    }
    Ok(())
}

/// A canonical decimal number: digits only, no leading zero except `0` itself.
fn canonical(digits: &str) -> Option<usize> {
    let ok = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'));
    if !ok {
        return None;
    }
    digits.parse().ok()
}
