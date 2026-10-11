//! `lines-v1`: exact codec for runs of byte-identical consecutive lines.
//! [`decode_lines`] also decodes `lines-v2`. The template directive lives in
//! `templates.rs`.
//!
//! The format is specified in `docs/compression-formats.md`.

use crate::templates::{self, HEADER_V2, TEMPLATE};

/// Stable transform identity for the line-run codec.
pub const EXACT_LINE_RUNS_ID: &str = "exact-line-runs";
/// Bumped whenever [`encode_lines`] output can change for some input.
pub const EXACT_LINE_RUNS_VERSION: u32 = 1;
/// Version of the `lines-v1` container.
pub const LINES_FORMAT_VERSION: u32 = 1;

/// Every body line that starts with this prefix is a directive or an escaped line.
pub(crate) const PREFIX: &str = "#!deslag";
pub(crate) const HEADER: &str = "#!deslag lines-v1\n";
pub(crate) const ESCAPE: &str = "#!deslag|";
const RUN: &str = "#!deslag x";

/// Why [`decode_lines`] rejected its input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    MissingHeader,
    /// A reserved-prefix line that is neither a valid escape nor a canonical run
    /// directive, or a directive with no unit directly above it. For
    /// `lines-v2` it also covers a non-canonical template directive, bad
    /// positions, missing rows, and a row with the wrong value count, an empty
    /// value, a tab in a value, a missing `\n`, or the reserved prefix without
    /// the escape.
    InvalidDirective,
    /// The decoded output would exceed the caller's bound.
    OutputTooLarge,
}

/// Encodes `input`, with no gain check. `None` when `input` already starts with
/// the `lines-v1` header.
pub fn encode_lines(input: &str) -> Option<String> {
    if input.starts_with(HEADER) {
        return None;
    }
    let mut out = String::from(HEADER);
    let mut units = input.split_inclusive('\n').peekable();
    while let Some(unit) = units.next() {
        let mut count = 1usize;
        while units.next_if_eq(&unit).is_some() {
            count += 1;
        }
        push_unit(&mut out, unit);
        if count < 2 {
            continue;
        }
        if let Some(directive) = run_directive(unit, count) {
            out.push_str(&directive);
        } else {
            for _ in 1..count {
                push_unit(&mut out, unit);
            }
        }
    }
    Some(out)
}

/// The run directive for `count` copies of `unit`, when it is shorter than the
/// `count - 1` copies it replaces.
pub(crate) fn run_directive(unit: &str, count: usize) -> Option<String> {
    let directive = format!("{RUN}{count}\n");
    (directive.len() < (count - 1) * unit_cost(unit)).then_some(directive)
}

/// Bytes `unit` costs when written out, escape included.
pub(crate) fn unit_cost(unit: &str) -> usize {
    unit.len()
        + if unit.starts_with(PREFIX) {
            ESCAPE.len()
        } else {
            0
        }
}

pub(crate) fn push_unit(out: &mut String, unit: &str) {
    if unit.starts_with(PREFIX) {
        out.push_str(ESCAPE);
    }
    out.push_str(unit);
}

/// Reconstructs the original text from `lines-v1` or `lines-v2` `encoded`. Fails
/// with [`DecodeError::OutputTooLarge`] before the output would pass `max_output` bytes.
pub fn decode_lines(encoded: &str, max_output: usize) -> Result<String, DecodeError> {
    let (v2, body) = match encoded.strip_prefix(HEADER) {
        Some(body) => (false, body),
        None => (
            true,
            encoded
                .strip_prefix(HEADER_V2)
                .ok_or(DecodeError::MissingHeader)?,
        ),
    };
    let mut out = String::new();
    // The unit directly above the next line, until a directive consumes it.
    let mut last: Option<&str> = None;
    let mut lines = body.split_inclusive('\n');
    while let Some(line) = lines.next() {
        let unit = if let Some(unit) = line.strip_prefix(ESCAPE) {
            if !unit.starts_with(PREFIX) {
                return Err(DecodeError::InvalidDirective);
            }
            unit
        } else if v2 && line.starts_with(TEMPLATE) {
            let base = last.take().ok_or(DecodeError::InvalidDirective)?;
            templates::decode_group(line, base, &mut lines, &mut out, max_output)?;
            continue;
        } else if line.starts_with(PREFIX) {
            let count = parse_run(line).ok_or(DecodeError::InvalidDirective)?;
            let unit = last.take().ok_or(DecodeError::InvalidDirective)?;
            push_bounded(&mut out, unit, count - 1, max_output)?;
            continue;
        } else {
            line
        };
        push_bounded(&mut out, unit, 1, max_output)?;
        last = Some(unit);
    }
    Ok(out)
}

/// The count N of a canonical `#!deslag x<N>\n` line, N >= 2.
fn parse_run(line: &str) -> Option<usize> {
    let digits = line.strip_prefix(RUN)?.strip_suffix('\n')?;
    if digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|&n| n >= 2)
}

pub(crate) fn push_bounded(
    out: &mut String,
    unit: &str,
    times: usize,
    max: usize,
) -> Result<(), DecodeError> {
    unit.len()
        .checked_mul(times)
        .and_then(|n| n.checked_add(out.len()))
        .filter(|&n| n <= max)
        .ok_or(DecodeError::OutputTooLarge)?;
    for _ in 0..times {
        out.push_str(unit);
    }
    Ok(())
}
