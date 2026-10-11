//! `lines-v1`: exact codec for runs of byte-identical consecutive lines.
//!
//! The format is specified in `docs/compression-formats.md`.

/// Stable transform identity for the line-run codec.
pub const EXACT_LINE_RUNS_ID: &str = "exact-line-runs";
/// Bumped whenever [`encode_lines`] output can change for some input.
pub const EXACT_LINE_RUNS_VERSION: u32 = 1;
/// Version of the `lines-v1` container.
pub const LINES_FORMAT_VERSION: u32 = 1;

/// Every body line that starts with this prefix is a directive or an escaped line.
const PREFIX: &str = "#!deslag";
const HEADER: &str = "#!deslag lines-v1\n";
const ESCAPE: &str = "#!deslag|";
const RUN: &str = "#!deslag x";

/// Why [`decode_lines`] rejected its input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    MissingHeader,
    /// A reserved-prefix line that is neither a valid escape nor a canonical run
    /// directive, or a directive with no unit directly above it.
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
        let directive = format!("{RUN}{count}\n");
        if directive.len() < (count - 1) * unit.len() {
            out.push_str(&directive);
        } else {
            for _ in 1..count {
                push_unit(&mut out, unit);
            }
        }
    }
    Some(out)
}

fn push_unit(out: &mut String, unit: &str) {
    if unit.starts_with(PREFIX) {
        out.push_str(ESCAPE);
    }
    out.push_str(unit);
}

/// Reconstructs the original text from `lines-v1` `encoded`. Fails with
/// [`DecodeError::OutputTooLarge`] before the output would pass `max_output` bytes.
pub fn decode_lines(encoded: &str, max_output: usize) -> Result<String, DecodeError> {
    let body = encoded
        .strip_prefix(HEADER)
        .ok_or(DecodeError::MissingHeader)?;
    let mut out = String::new();
    // The unit directly above the next line, until a directive consumes it.
    let mut last: Option<&str> = None;
    for line in body.split_inclusive('\n') {
        let unit = if let Some(unit) = line.strip_prefix(ESCAPE) {
            if !unit.starts_with(PREFIX) {
                return Err(DecodeError::InvalidDirective);
            }
            unit
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

fn push_bounded(out: &mut String, unit: &str, times: usize, max: usize) -> Result<(), DecodeError> {
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
