//! table-v1: a lossless CSV-schema encoding for a JSON array of objects.
//! Uniform nested objects flatten to dotted columns such as `meta.region`.
//!
//! `docs/compression-formats.md` specifies the format. The round-trip contract
//! is `decode(&encode(x)?) == minified(x)`, byte for byte.
//! [`render_markdown_kv`] renders the same rows as Markdown-KV for evaluation
//! only; nothing in the compression context selects it.

use std::{collections::HashMap, fmt::Write, iter};

use crate::{Limits, Reason, Scanner, TokenCounter, minify};

pub mod v2;

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
    /// Counts tokens for the token gain check. `None` skips that check.
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

/// Column type. A column of any type may also hold `null`. Only an undotted
/// column may hold missing cells.
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

/// Fields in order, keyed by the [`Paths`] node id of each leaf path.
type Row<'a> = Vec<(usize, Value<'a>)>;

/// Every key path seen, as a tree keyed by (parent node id, key), so each key
/// costs O(1) instead of its joined path length. Node 0 is the root.
struct Paths<'a> {
    ids: HashMap<(usize, &'a str), usize>,
    /// Parent id, key and joined path length per node, indexed by id.
    nodes: Vec<(usize, &'a str, usize)>,
    /// Joined bytes of all distinct leaf paths. Every leaf path becomes a
    /// header column. Once this exceeds the input, the encoder passes the
    /// input through with `unsupported_format` before any gain check runs.
    leaf_bytes: usize,
}

impl<'a> Paths<'a> {
    /// Returns the id of `key` under `parent` and whether it is new.
    fn node(&mut self, parent: usize, key: &'a str) -> (usize, bool) {
        let next = self.nodes.len();
        let id = *self.ids.entry((parent, key)).or_insert(next);
        if id == next {
            let len = if parent == 0 {
                key.len()
            } else {
                self.nodes[parent].2 + 1 + key.len()
            };
            self.nodes.push((parent, key, len));
        }
        (id, id == next)
    }

    /// The dotted path of node `id`, such as `meta.region`.
    fn join(&self, mut id: usize) -> String {
        let mut keys = Vec::new();
        while id != 0 {
            keys.push(self.nodes[id].1);
            id = self.nodes[id].0;
        }
        keys.reverse();
        keys.join(".")
    }
}

/// Encodes `input` as table-v1. `Err` means: pass the input through unchanged.
pub fn encode(input: &str, opts: &TableOptions) -> Result<String, Reason> {
    build(input, opts, false)
}

/// Renders `input` as Markdown-KV, for evaluation only: the table-v1 header,
/// then one list item per row with a `key: value` line per present field.
/// `None` exactly when [`encode`] fails. There is no decoder.
pub fn render_markdown_kv(input: &str, opts: &TableOptions) -> Option<String> {
    build(input, opts, true).ok()
}

/// Checks eligibility and gain on the CSV form, then returns the CSV form or,
/// with `kv`, the Markdown-KV form of the same rows.
fn build(input: &str, opts: &TableOptions, kv: bool) -> Result<String, Reason> {
    const NO: Reason = Reason::UnsupportedFormat;
    if input.trim_start().starts_with(MARKER) {
        return Err(NO);
    }
    let minified = String::from_utf8(minify(input, opts.max_depth)?).map_err(|_| NO)?;
    let (rows, paths) = parse_rows(&minified).ok_or(NO)?;
    let mut out = format!("{MARKER} ");
    push_block(&mut out, &rows, &paths, opts.max_rows).ok_or(NO)?;
    if !enough_gain(&minified, &out, opts) {
        return Err(Reason::InsufficientGain);
    }
    if kv {
        // Column names hold no line break, so the header is the first line.
        out.truncate(out.find('\n').unwrap_or(out.len()));
        for row in &rows {
            out.push_str("\n-");
            for (j, (key, v)) in row.iter().enumerate() {
                out.push_str(if j == 0 { " " } else { "\n  " });
                out.push_str(&paths.join(*key));
                out.push_str(": ");
                // Each line break (`\r\n`, lone `\r` or `\n`) becomes `\n` plus the
                // item indent, so value text cannot start a new top-level row item.
                let mut cell = String::new();
                push_cell(&mut cell, v);
                let cell = cell.replace("\r\n", "\n").replace('\r', "\n");
                out.push_str(&cell.replace('\n', "\n  "));
            }
        }
    }
    Ok(out)
}

/// Encodes one minified JSON array as a block: the `[count]{columns}` header
/// and its records. `None` when the array is ineligible.
fn encode_block(array: &str, max_rows: usize) -> Option<String> {
    let (rows, paths) = parse_rows(array)?;
    let mut out = String::new();
    push_block(&mut out, &rows, &paths, max_rows)?;
    Some(out)
}

/// Writes the `[count]{columns}` header and one record per row. `None` when
/// the rows are ineligible.
fn push_block(out: &mut String, rows: &[Row], paths: &Paths, max_rows: usize) -> Option<()> {
    if rows.len() < 2 || rows.len() > max_rows {
        return None;
    }
    let (cols, names, types) = layout(rows, paths)?;
    let _ = write!(out, "[{}]{{", rows.len());
    for (j, (col, ty)) in names.iter().zip(&types).enumerate() {
        let sep = if j == 0 { "" } else { "," };
        let _ = write!(out, "{sep}{col}:{}", ty.name());
    }
    out.push('}');
    for row in rows {
        out.push('\n');
        // Row keys are a subsequence of `cols`, so one pass aligns them.
        let mut fields = row.iter().peekable();
        for (j, col) in cols.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            if let Some((_, v)) = fields.next_if(|(k, _)| k == col) {
                push_cell(out, v);
            }
        }
    }
    Some(())
}

/// Writes a present field as a cell; a missing field is the empty cell.
fn push_cell(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Num(n) => out.push_str(n),
        Value::Str(s) => push_str_cell(out, s),
    }
}

/// Decodes table-v1 text to minified JSON. `None` for malformed text.
pub fn decode(text: &str) -> Option<String> {
    let block = text.strip_prefix(MARKER)?.strip_prefix(' ')?;
    let mut i = 0;
    let out = decode_block(block, &mut i)?;
    (i == block.len()).then_some(out)
}

/// Decodes the block at `*i` of `text` to a minified JSON array and moves
/// `*i` to the end of its last record. A block is a `[count]{columns}` header
/// line and `count` records, each after a `\n`.
fn decode_block(text: &str, i: &mut usize) -> Option<String> {
    let header_end = text[*i..].find('\n').map_or(text.len(), |e| *i + e);
    let (count, specs) = text[*i..header_end].strip_prefix('[')?.split_once("]{")?;
    // `0` or `[1-9][0-9]*`: one canonical spelling per count.
    if count.is_empty()
        || !count.bytes().all(|c| c.is_ascii_digit())
        || (count.len() > 1 && count.starts_with('0'))
    {
        return None;
    }
    let rows: usize = count.parse().ok()?;
    let cols = columns(specs.strip_suffix('}')?)?;
    *i = header_end;

    let mut out = String::from("[");
    for r in 0..rows {
        if !text[*i..].starts_with('\n') {
            return None;
        }
        *i += 1;
        let cells = parse_record(text, i)?;
        if cells.len() != cols.len() {
            return None;
        }
        out.push_str(if r == 0 { "{" } else { ",{" });
        // True right after `{`, where the next member needs no comma.
        let mut opened = true;
        for (cell, col) in cells.iter().zip(&cols) {
            if col.close > 0 {
                out.extend(iter::repeat_n('}', col.close));
                opened = false;
            }
            if matches!(cell, Cell::Bare("")) {
                // Only a top-level field can be missing.
                if col.depth > 0 {
                    return None;
                }
                continue;
            }
            for name in &col.open {
                push_key(&mut out, opened, name);
                out.push('{');
                opened = true;
            }
            push_key(&mut out, opened, col.leaf);
            push_json_value(&mut out, cell, col.ty)?;
            opened = false;
        }
        out.extend(iter::repeat_n('}', cols.last().map_or(0, |c| c.depth)));
        out.push('}');
    }
    out.push(']');
    Some(out)
}

fn push_key(out: &mut String, opened: bool, name: &str) {
    out.push_str(if opened { "\"" } else { ",\"" });
    out.push_str(name);
    out.push_str("\":");
}

/// A header column, with the nested-object steps to take before its cell.
struct Column<'t> {
    /// Nested objects to close, innermost first.
    close: usize,
    /// Nested objects to open, outermost first.
    open: Vec<&'t str>,
    /// Last path segment: the member name of the cell.
    leaf: &'t str,
    /// Nesting depth of the cell, 0 for a top-level field.
    depth: usize,
    ty: Ty,
}

/// Parses the header column list. `None` unless the paths form one tree in
/// which every path is unique, no path is both a leaf and an object, and each
/// object's columns are contiguous.
fn columns(specs: &str) -> Option<Vec<Column<'_>>> {
    let mut cols = Vec::new();
    // Every object and leaf seen so far, keyed by (parent node id, name), so
    // each segment costs O(1) and parsing is linear in header length.
    // Node 0 is the root.
    let mut nodes: HashMap<(usize, &str), usize> = HashMap::new();
    // Open objects, outermost first, with their node ids.
    let mut stack: Vec<(&str, usize)> = Vec::new();
    for spec in specs.split(',') {
        let (path, ty) = spec.rsplit_once(':')?;
        if !valid_path(path) {
            return None;
        }
        let segs: Vec<&str> = path.split('.').collect();
        if segs.len() >= crate::MAX_JSON_DEPTH_CEILING {
            return None;
        }
        let (leaf, groups) = segs.split_last()?;
        let common = stack
            .iter()
            .zip(groups)
            .take_while(|((a, _), b)| a == *b)
            .count();
        let close = stack.len() - common;
        stack.truncate(common);
        let mut parent = stack.last().map_or(0, |&(_, id)| id);
        for &name in &groups[common..] {
            // A reopened object is either a duplicate or not contiguous.
            parent = insert_node(&mut nodes, parent, name)?;
            stack.push((name, parent));
        }
        insert_node(&mut nodes, parent, leaf)?;
        cols.push(Column {
            close,
            open: groups[common..].to_vec(),
            leaf,
            depth: groups.len(),
            ty: Ty::parse(ty)?,
        });
    }
    Some(cols)
}

/// Adds `name` under `parent` and returns its new node id. `None` when the
/// node already exists.
fn insert_node<'t>(
    nodes: &mut HashMap<(usize, &'t str), usize>,
    parent: usize,
    name: &'t str,
) -> Option<usize> {
    let id = nodes.len() + 1;
    nodes.insert((parent, name), id).is_none().then_some(id)
}

/// Parses the minified array into rows of leaf path ids. `None` when the top
/// level is not an array, an item is not an object, a field holds an array,
/// nested objects are not uniform across rows, or the distinct leaf paths
/// together are longer than `min`.
fn parse_rows(min: &str) -> Option<(Vec<Row<'_>>, Paths<'_>)> {
    let mut s = Scanner {
        b: min.as_bytes(),
        i: 0,
        out: Vec::new(),
        max_depth: 0,
    };
    let mut paths = Paths {
        ids: HashMap::new(),
        nodes: vec![(0, "", 0)],
        leaf_bytes: 0,
    };
    s.expect(b'[').ok()?;
    let mut rows = Vec::new();
    if s.eat(b']') {
        return Some((rows, paths));
    }
    let mut first_shape = None;
    loop {
        let (mut row, mut shape) = (Vec::new(), Vec::new());
        parse_object(&mut s, min, 0, &mut row, &mut shape, &mut paths)?;
        // Every row must have the same nested objects with the same key lists.
        if *first_shape.get_or_insert_with(|| shape.clone()) != shape {
            return None;
        }
        rows.push(row);
        if !s.eat(b',') {
            break;
        }
    }
    s.expect(b']').ok()?;
    Some((rows, paths))
}

/// Parses the object at path node `parent` into `row`, flattening nested
/// objects into leaf paths. `parent` is 0 for a row. `shape` collects, in
/// order, each nested object's node (`true`) and each key node inside a
/// nested object (`false`).
fn parse_object<'a>(
    s: &mut Scanner<'a>,
    min: &'a str,
    parent: usize,
    row: &mut Row<'a>,
    shape: &mut Vec<(bool, usize)>,
    paths: &mut Paths<'a>,
) -> Option<()> {
    s.expect(b'{').ok()?;
    if s.eat(b'}') {
        // A nested `{}` would leave no column to restore it from.
        return (parent == 0).then_some(());
    }
    loop {
        let start = s.i;
        s.string().ok()?;
        // The scanner rejected escaped keys, so the lexeme is the key.
        let key = &min[start + 1..s.i - 1];
        if !valid_name(key) {
            return None;
        }
        let (id, new) = paths.node(parent, key);
        if parent != 0 {
            shape.push((false, id));
        }
        s.expect(b':').ok()?;
        let start = s.i;
        let value = match s.peek()? {
            b'{' => {
                shape.push((true, id));
                parse_object(s, min, id, row, shape, paths)?;
                None
            }
            b'"' => {
                s.string().ok()?;
                Some(Value::Str(canonical_text(&min[start + 1..s.i - 1])?))
            }
            b't' => {
                s.literal(b"true").ok()?;
                Some(Value::Bool(true))
            }
            b'f' => {
                s.literal(b"false").ok()?;
                Some(Value::Bool(false))
            }
            b'n' => {
                s.literal(b"null").ok()?;
                Some(Value::Null)
            }
            b'-' | b'0'..=b'9' => {
                s.number().ok()?;
                Some(Value::Num(&min[start..s.i]))
            }
            _ => return None,
        };
        if let Some(value) = value {
            if new {
                // Leaf-path byte budget: a header longer than the input
                // passes through as `unsupported_format`.
                paths.leaf_bytes += paths.nodes[id].2;
                if paths.leaf_bytes > min.len() {
                    return None;
                }
            }
            row.push((id, value));
        }
        if !s.eat(b',') {
            break;
        }
    }
    s.expect(b'}').ok()
}

/// Column node ids in first-seen order, their dotted names and their types.
/// `None` when a row's key order is not a subsequence of the column order, a
/// key is not a valid column name, there are no columns, or a column mixes
/// non-null types.
fn layout(rows: &[Row], paths: &Paths) -> Option<(Vec<usize>, Vec<String>, Vec<Ty>)> {
    let mut index: HashMap<usize, usize> = HashMap::new();
    let mut cols = Vec::new();
    for &(key, _) in rows.iter().flatten() {
        index.entry(key).or_insert_with(|| {
            cols.push(key);
            cols.len() - 1
        });
    }
    // Joined once per column. `parse_rows` bounds the total by the input.
    let names: Vec<String> = cols.iter().map(|&c| paths.join(c)).collect();
    if cols.is_empty() || !names.iter().all(|c| valid_path(c)) {
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
    Some((cols, names, types))
}

/// The encoded text, header included, must save at least
/// `min_savings_permille` of the minified bytes and, with a token counter,
/// at least `min_token_savings` tokens.
fn enough_gain(minified: &str, encoded: &str, opts: &TableOptions) -> bool {
    // u128 keeps `permille * m` from overflowing for any option value.
    let (m, e) = (minified.len() as u128, encoded.len() as u128);
    if e >= m || (m - e) * 1000 < u128::from(opts.min_savings_permille) * m {
        return false;
    }
    opts.token_counter.is_none_or(|c| {
        // checked_sub: a token regression fails even when the minimum is zero.
        c.count(minified)
            .count
            .checked_sub(c.count(encoded).count)
            .is_some_and(|saved| saved >= opts.min_token_savings)
    })
}

/// A column name must survive the header and CSV syntax and need no JSON escape.
fn valid_name(name: &str) -> bool {
    !name
        .chars()
        .any(|c| matches!(c, '.' | ',' | '"' | '\\') || c < ' ')
}

/// A column path is one name, or two or more non-empty names joined by `.`.
fn valid_path(path: &str) -> bool {
    valid_name(path) || path.split('.').all(|s| !s.is_empty() && valid_name(s))
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
