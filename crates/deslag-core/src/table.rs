//! table-v1: a lossless CSV-schema encoding for a JSON array of flat objects.
//!
//! `docs/compression-formats.md` specifies the format. The round-trip contract
//! is `decode(&encode(x)?) == minified(x)`, byte for byte.

use std::{collections::HashMap, fmt::Write};

use crate::{Limits, Reason, Scanner, TokenCounter, minify};

/// First token of every table-v1 header line.
pub const MARKER: &str = "table-v1";

/// Eligibility and gain thresholds for [`encode`].
#[derive(Clone, Copy)]
pub struct TableOptions<'c> {
    /// Arrays with more rows pass through.
    pub max_rows: usize,
    /// Minimum byte savings against the minified original, in permille.
    pub min_savings_permille: u64,
    /// Minimum token savings. Applies only when `token_counter` is set.
    pub min_token_savings: u64,
    /// Containers nested deeper pass through with `DepthLimit`.
    pub max_depth: usize,
    pub token_counter: Option<&'c dyn TokenCounter>,
}

impl Default for TableOptions<'_> {
    /// Proposed starting values from the rollout policy.
    fn default() -> Self {
        TableOptions {
            max_rows: 20_000,
            min_savings_permille: 100,
            min_token_savings: 64,
            max_depth: Limits::default().max_json_depth,
            token_counter: None,
        }
    }
}

/// Column type. A column of any type may also hold `null` or missing cells.
#[derive(Clone, Copy, PartialEq)]
enum Ty {
    Int,
    Num,
    Str,
    Bool,
}

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Ty::Int => "int",
            Ty::Num => "num",
            Ty::Str => "str",
            Ty::Bool => "bool",
        }
    }

    fn parse(s: &str) -> Option<Ty> {
        [Ty::Int, Ty::Num, Ty::Str, Ty::Bool]
            .into_iter()
            .find(|t| t.name() == s)
    }
}

enum Value<'a> {
    Null,
    Bool(bool),
    /// Raw number lexeme.
    Num(&'a str),
    /// Decoded text whose canonical encoding equals the raw lexeme.
    Str(String),
}

type Row<'a> = Vec<(&'a str, Value<'a>)>;

/// Encodes `input` as table-v1. `Err` means: pass the input through unchanged.
pub fn encode(input: &str, opts: &TableOptions) -> Result<String, Reason> {
    const NO: Reason = Reason::UnsupportedFormat;
    if input.trim_start().starts_with(MARKER) {
        return Err(NO);
    }
    let minified = String::from_utf8(minify(input, opts.max_depth)?).map_err(|_| NO)?;
    let rows = parse_rows(&minified).ok_or(NO)?;
    if rows.len() < 2 || rows.len() > opts.max_rows {
        return Err(NO);
    }
    let (cols, types) = layout(&rows).ok_or(NO)?;

    let mut out = format!("{MARKER} [{}]{{", rows.len());
    for (j, (col, ty)) in cols.iter().zip(&types).enumerate() {
        let sep = if j == 0 { "" } else { "," };
        let _ = write!(out, "{sep}{col}:{}", ty.name());
    }
    out.push('}');
    for row in &rows {
        out.push('\n');
        // Row keys are a subsequence of `cols`, so one pass aligns them.
        let mut fields = row.iter().peekable();
        for (j, col) in cols.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            match fields.next_if(|(k, _)| k == col).map(|(_, v)| v) {
                None => {}
                Some(Value::Null) => out.push_str("null"),
                Some(Value::Bool(b)) => out.push_str(if *b { "true" } else { "false" }),
                Some(Value::Num(n)) => out.push_str(n),
                Some(Value::Str(s)) => push_str_cell(&mut out, s),
            }
        }
    }
    if !enough_gain(&minified, &out, opts) {
        return Err(Reason::InsufficientGain);
    }
    Ok(out)
}

/// Decodes table-v1 text to minified JSON. `None` for malformed text.
pub fn decode(text: &str) -> Option<String> {
    let (header, body) = match text.split_once('\n') {
        Some((h, b)) => (h, Some(b)),
        None => (text, None),
    };
    let (count, specs) = header
        .strip_prefix(MARKER)?
        .strip_prefix(" [")?
        .split_once("]{")?;
    if count.is_empty() || !count.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let rows: usize = count.parse().ok()?;
    let mut cols: Vec<(&str, Ty)> = Vec::new();
    for spec in specs.strip_suffix('}')?.split(',') {
        let (name, ty) = spec.rsplit_once(':')?;
        if !valid_name(name) || cols.iter().any(|(c, _)| *c == name) {
            return None;
        }
        cols.push((name, Ty::parse(ty)?));
    }
    // A header with zero rows has no body; every row is one line after it.
    if (rows == 0) != body.is_none() {
        return None;
    }
    let body = body.unwrap_or("");

    let mut out = String::from("[");
    let mut i = 0;
    for r in 0..rows {
        let cells = parse_record(body, &mut i)?;
        let last = r + 1 == rows;
        let at_end = if last {
            i == body.len()
        } else {
            body[i..].starts_with('\n')
        };
        if cells.len() != cols.len() || !at_end {
            return None;
        }
        i += 1;
        out.push_str(if r == 0 { "{" } else { ",{" });
        let mut first = true;
        for (cell, (name, ty)) in cells.iter().zip(&cols) {
            if matches!(cell, Cell::Bare("")) {
                continue;
            }
            out.push_str(if first { "\"" } else { ",\"" });
            first = false;
            out.push_str(name);
            out.push_str("\":");
            push_json_value(&mut out, cell, *ty)?;
        }
        out.push('}');
    }
    out.push(']');
    Some(out)
}

/// Parses the minified array into flat rows. `None` when the top level is not
/// an array, an item is not an object, or a field holds an array or object.
fn parse_rows(min: &str) -> Option<Vec<Row<'_>>> {
    let mut s = Scanner {
        b: min.as_bytes(),
        i: 0,
        out: Vec::new(),
        max_depth: 0,
    };
    s.expect(b'[').ok()?;
    let mut rows = Vec::new();
    if s.eat(b']') {
        return Some(rows);
    }
    loop {
        s.expect(b'{').ok()?;
        let mut row = Vec::new();
        if !s.eat(b'}') {
            loop {
                let start = s.i;
                s.string().ok()?;
                // The scanner rejected escaped keys, so the lexeme is the key.
                let key = &min[start + 1..s.i - 1];
                s.expect(b':').ok()?;
                let start = s.i;
                let value = match s.peek()? {
                    b'"' => {
                        s.string().ok()?;
                        Value::Str(canonical_text(&min[start + 1..s.i - 1])?)
                    }
                    b't' => {
                        s.literal(b"true").ok()?;
                        Value::Bool(true)
                    }
                    b'f' => {
                        s.literal(b"false").ok()?;
                        Value::Bool(false)
                    }
                    b'n' => {
                        s.literal(b"null").ok()?;
                        Value::Null
                    }
                    b'-' | b'0'..=b'9' => {
                        s.number().ok()?;
                        Value::Num(&min[start..s.i])
                    }
                    _ => return None,
                };
                row.push((key, value));
                if !s.eat(b',') {
                    break;
                }
            }
            s.expect(b'}').ok()?;
        }
        rows.push(row);
        if !s.eat(b',') {
            break;
        }
    }
    s.expect(b']').ok()?;
    Some(rows)
}

/// Columns in first-seen order and their types. `None` when a row's key order
/// is not a subsequence of the column order, a key is not a valid column name,
/// there are no columns, or a column mixes non-null types.
fn layout<'a>(rows: &[Row<'a>]) -> Option<(Vec<&'a str>, Vec<Ty>)> {
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut cols = Vec::new();
    for (key, _) in rows.iter().flatten() {
        index.entry(key).or_insert_with(|| {
            cols.push(*key);
            cols.len() - 1
        });
    }
    if cols.is_empty() || !cols.iter().all(|c| valid_name(c)) {
        return None;
    }
    let mut kinds: Vec<Option<Ty>> = vec![None; cols.len()];
    for row in rows {
        let mut prev = None;
        for (key, value) in row {
            let j = index[key];
            if prev.is_some_and(|p| j <= p) {
                return None;
            }
            prev = Some(j);
            let ty = match value {
                Value::Null => continue,
                Value::Bool(_) => Ty::Bool,
                Value::Num(n) if is_int(n) => Ty::Int,
                Value::Num(_) => Ty::Num,
                Value::Str(_) => Ty::Str,
            };
            kinds[j] = Some(match (kinds[j], ty) {
                (None, t) => t,
                (Some(a), b) if a == b => a,
                (Some(Ty::Int | Ty::Num), Ty::Int | Ty::Num) => Ty::Num,
                _ => return None,
            });
        }
    }
    let types = kinds.into_iter().map(|k| k.unwrap_or(Ty::Str)).collect();
    Some((cols, types))
}

/// The encoded text, header included, must save at least
/// `min_savings_permille` of the minified bytes and, with a token counter,
/// at least `min_token_savings` tokens.
fn enough_gain(minified: &str, encoded: &str, opts: &TableOptions) -> bool {
    let (m, e) = (minified.len() as u64, encoded.len() as u64);
    if e >= m || (m - e) * 1000 < opts.min_savings_permille * m {
        return false;
    }
    opts.token_counter.is_none_or(|c| {
        let saved = c
            .count(minified)
            .count
            .saturating_sub(c.count(encoded).count);
        saved >= opts.min_token_savings
    })
}

/// A column name must survive the header and CSV syntax and need no JSON escape.
fn valid_name(name: &str) -> bool {
    !name
        .chars()
        .any(|c| matches!(c, '.' | ',' | '"' | '\\') || c < ' ')
}

/// Quotes a string cell when its bare text would read as missing, `null`, a
/// boolean or a number, or contains CSV metacharacters.
fn push_str_cell(out: &mut String, text: &str) {
    // ponytail: f64 parsing also accepts `inf`, `NaN` and `+1`; quoting those
    // too is harmless and keeps the rule one call.
    let quote = text.is_empty()
        || matches!(text, "null" | "true" | "false")
        || text.parse::<f64>().is_ok()
        || text.contains([',', '"', '\n', '\r']);
    if quote {
        out.push('"');
        out.push_str(&text.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(text);
    }
}

enum Cell<'t> {
    /// Unquoted text; the empty cell means a missing field.
    Bare(&'t str),
    /// Quoted text with `""` undoubled; always a string.
    Quoted(String),
}

/// Parses one RFC 4180 record starting at `*i`. Stops at `\n` or the end.
fn parse_record<'t>(body: &'t str, i: &mut usize) -> Option<Vec<Cell<'t>>> {
    let mut cells = Vec::new();
    loop {
        if body[*i..].starts_with('"') {
            let mut text = String::new();
            let mut start = *i + 1;
            loop {
                let q = start + body[start..].find('"')?;
                text.push_str(&body[start..q]);
                if body[q + 1..].starts_with('"') {
                    text.push('"');
                    start = q + 2;
                } else {
                    *i = q + 1;
                    break;
                }
            }
            cells.push(Cell::Quoted(text));
        } else {
            let end = body[*i..].find([',', '\n']).map_or(body.len(), |e| *i + e);
            let text = &body[*i..end];
            if text.contains(['"', '\r']) {
                return None;
            }
            cells.push(Cell::Bare(text));
            *i = end;
        }
        if !body[*i..].starts_with(',') {
            return Some(cells);
        }
        *i += 1;
    }
}

/// Writes a non-missing cell as JSON, interpreted by the column type.
fn push_json_value(out: &mut String, cell: &Cell, ty: Ty) -> Option<()> {
    match (cell, ty) {
        (Cell::Quoted(s), Ty::Str) => push_json_str(out, s),
        (Cell::Quoted(_), _) => return None,
        (Cell::Bare(s @ "null"), _) => out.push_str(s),
        (Cell::Bare(s), Ty::Int) if is_int(s) => out.push_str(s),
        (Cell::Bare(s), Ty::Num) if is_number(s) => out.push_str(s),
        (Cell::Bare(s @ ("true" | "false")), Ty::Bool) => out.push_str(s),
        (Cell::Bare(s), Ty::Str) => push_json_str(out, s),
        _ => return None,
    }
    Some(())
}

fn is_number(s: &str) -> bool {
    let mut sc = Scanner {
        b: s.as_bytes(),
        i: 0,
        out: Vec::new(),
        max_depth: 0,
    };
    sc.number().is_ok() && sc.i == s.len()
}

/// `-?(0|[1-9][0-9]*)`.
fn is_int(s: &str) -> bool {
    is_number(s) && !s.contains(['.', 'e', 'E'])
}

/// Canonical JSON string encoding: escapes `"`, `\`, the five short control
/// escapes, other controls as lowercase `\u00xx`, and nothing else.
fn push_json_str(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Decodes the body of a valid string lexeme. `None` unless re-encoding the
/// text with [`push_json_str`] yields the same lexeme, so the decoder can
/// restore it exactly.
fn canonical_text(raw: &str) -> Option<String> {
    let mut text = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }
        text.push(match chars.next()? {
            'b' => '\u{8}',
            'f' => '\u{c}',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'u' => {
                let hex: String = chars.by_ref().take(4).collect();
                // Surrogates and non-control escapes are never canonical.
                char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?
            }
            c => c,
        });
    }
    let mut again = String::with_capacity(raw.len() + 2);
    push_json_str(&mut again, &text);
    (again[1..again.len() - 1] == *raw).then_some(text)
}
