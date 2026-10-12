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
pub const LINE_TEMPLATES_VERSION: u32 = 2;
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
    let parts: Vec<Option<Vec<&str>>> = units
        .iter()
        .map(|u| u.strip_suffix('\n').map(split))
        .collect();
    let mut out = String::from(HEADER_V2);
    let mut i = 0;
    while i < units.len() {
        let unit = units[i];
        let count = run_length(&units[i..]);
        if count >= 2
            && let Some(directive) = run_directive(unit, count)
        {
            push_unit(&mut out, unit);
            out.push_str(&directive);
            i += count;
            continue;
        }
        if let Some((len, group)) = template_group(&units[i..], &parts[i..]) {
            out.push_str(&group);
            i += len;
            continue;
        }
        push_unit(&mut out, unit);
        i += 1;
    }
    Some(out)
}

// ponytail: a group attempt scans at most WINDOW rows before it must commit, so
// encoding is O(WINDOW * input). A shorter group past the window that would pay
// more is not found; raise WINDOW or segment optimally (DP) if that matters.
const WINDOW: usize = 16;

/// Grows a group from `units[0]` and returns the length and encoding of the
/// prefix that saves the most bytes, or `None` when no prefix pays. `parts`
/// holds the split content of each unit, `None` for a partial unit. Rows join
/// while they fit the base and do not start a run that is better as a run
/// directive. Past `WINDOW` rows, a row joins only when the current end is the
/// best end and the row adds no variable index.
fn template_group(units: &[&str], parts: &[Option<Vec<&str>>]) -> Option<(usize, String)> {
    let base = parts[0].as_deref()?;
    let mut variable = vec![false; base.len()];
    let mut bytes = vec![0; base.len()];
    let mut escapes = vec![0; base.len()];
    let (mut rows, mut literal, mut best) = (Vec::new(), 0, None);
    // Once every token is variable, each further row saves its gap bytes plus
    // the newline minus one separator per token. When that is not positive and
    // no prefix pays yet, no longer prefix can pay either.
    let gaps: usize = base.iter().step_by(2).map(|g| g.len()).sum();
    let dead_end = gaps < base.len() / 2;
    for (j, (&unit, row)) in units.iter().zip(parts).enumerate().skip(1) {
        let Some(row) = row.as_deref().filter(|row| fits(base, row)) else {
            break;
        };
        let grows = (1..base.len())
            .step_by(2)
            .any(|k| !variable[k] && row[k] != base[k]);
        if units[j - 1] != unit && run_wins(&units[j..], row, &variable, grows) {
            break;
        }
        if rows.len() >= WINDOW && (grows || best.map(|(n, _)| n) != Some(rows.len())) {
            break;
        }
        for k in (1..base.len()).step_by(2) {
            variable[k] |= row[k] != base[k];
            bytes[k] += row[k].len();
            escapes[k] += usize::from(row[k].starts_with(PREFIX));
        }
        literal += unit_cost(unit);
        rows.push(row);
        let n = rows.len();
        let positions = (1..base.len()).step_by(2).filter(|&k| variable[k]);
        let (mut m, mut cost) = (0, TEMPLATE.len() + digits(n + 1) + 1);
        for k in positions.clone() {
            m += 1;
            cost += digits(k / 2) + 1 + bytes[k];
        }
        if let Some(first) = positions.clone().next() {
            cost += n * m + escapes[first] * ESCAPE.len();
            if cost < literal && best.is_none_or(|(_, saving)| literal - cost >= saving) {
                best = Some((n, literal - cost));
            }
        }
        if best.is_none() && dead_end && variable.iter().skip(1).step_by(2).all(|&v| v) {
            break;
        }
    }
    let (n, saving) = best?;
    let rows = &rows[..n];
    let positions: Vec<usize> = (1..base.len())
        .step_by(2)
        .filter(|&k| rows.iter().any(|row| row[k] != base[k]))
        .collect();
    let list: Vec<String> = positions.iter().map(|k| (k / 2).to_string()).collect();
    let mut body = format!("{TEMPLATE}{} {}\n", n + 1, list.join(","));
    for row in rows {
        let values: Vec<&str> = positions.iter().map(|&k| row[k]).collect();
        push_unit(&mut body, &(values.join(" ") + "\n"));
    }
    let literal: usize = units[1..=n].iter().map(|u| unit_cost(u)).sum();
    debug_assert_eq!(body.len(), literal - saving, "group cost estimate drifted");
    if body.len() >= literal {
        return None;
    }
    let mut out = String::new();
    push_unit(&mut out, units[0]);
    out.push_str(&body);
    Some((n + 1, out))
}

/// Number of identical consecutive units at the start of `units`.
fn run_length(units: &[&str]) -> usize {
    units.iter().take_while(|&&u| u == units[0]).count()
}

/// True when the run that starts at `units[0]` (split as `row`) saves more as a
/// run directive than as group rows at the current `variable` indices. A row
/// that `grows` the variable indices always yields to a paying run.
fn run_wins(units: &[&str], row: &[&str], variable: &[bool], grows: bool) -> bool {
    let unit = units[0];
    let count = run_length(units);
    let Some(directive) = run_directive(unit, count) else {
        return false;
    };
    let positions: Vec<usize> = (1..row.len()).step_by(2).filter(|&k| variable[k]).collect();
    let Some(&first) = positions.first().filter(|_| !grows) else {
        return true;
    };
    let row_cost = positions.iter().map(|&k| row[k].len() + 1).sum::<usize>()
        + usize::from(row[first].starts_with(PREFIX)) * ESCAPE.len();
    let run_saving = (count - 1) * unit_cost(unit) - directive.len();
    run_saving > count * unit_cost(unit).saturating_sub(row_cost)
}

fn digits(n: usize) -> usize {
    n.checked_ilog10().map_or(1, |d| d as usize + 1)
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
