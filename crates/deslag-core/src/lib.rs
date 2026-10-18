//! Shared core for deslag.
//!
//! [`compress`] is the core contract. It is pure: the context carries no
//! network client, filesystem handle or session state. Adapters own decision
//! recording and decide whether to commit a `Changed` output.

mod bounded;
mod lines;
mod templates;

pub use bounded::{BoundedExecutor, Rejected};
pub use lines::{
    DecodeError, EXACT_LINE_RUNS_ID, EXACT_LINE_RUNS_VERSION, LINES_FORMAT_VERSION, decode_lines,
    encode_lines,
};
pub use templates::{
    LINE_TEMPLATES_ID, LINE_TEMPLATES_VERSION, LINES_V2_FORMAT_VERSION, encode_templates,
};

use std::{borrow::Cow, collections::HashSet, fmt::Write, time::Duration, time::Instant};

pub mod table;
mod tokens;

pub use table::TableOptions;
pub use tokens::EstimateCounter;

/// Crate version, as set in the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Stable transform identity for the JSON minifier.
pub const JSON_MINIFY_ID: &str = "json-minify";
/// Bumped whenever `minify_json` output can change for some input.
pub const JSON_MINIFY_VERSION: u32 = 1;
/// Stable transform identity for the table encoder.
pub const JSON_TABLE_ID: &str = "json-table";
/// Bumped whenever `table::v2::encode` output can change for some input.
pub const JSON_TABLE_VERSION: u32 = 5;
/// Format version of table-v1 output, as reported in `format_version`.
pub const JSON_TABLE_FORMAT_VERSION: u32 = 1;
/// Format version of table-v2 output, as reported in `format_version`.
pub const JSON_TABLE_V2_FORMAT_VERSION: u32 = 2;
/// Bumped whenever a [`CompressionReport`] field or its serialized form changes.
pub const REPORT_SCHEMA_VERSION: u32 = 1;

/// Policy mode: `Off` builds no candidate, `Observe` builds one but emits the
/// original, `Safe` emits a validated candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    Observe,
    Safe,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Observe => "observe",
            Mode::Safe => "safe",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The output is the candidate.
    Changed,
    /// Observe mode: a valid candidate exists, the output is the original.
    WouldChange,
    /// The output is the original; `reason` says why.
    Skipped,
    /// The candidate failed validation; the output is the original.
    Failed,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Changed => "changed",
            Outcome::WouldChange => "would_change",
            Outcome::Skipped => "skipped",
            Outcome::Failed => "failed",
        }
    }
}

/// Bounded skip or failure reason. Only `InvalidCandidate` pairs with `Failed`.
///
/// The first reason the scanner meets wins. `DepthLimit`, `EscapedKey` and
/// `DuplicateKey` are reported as soon as the scanner reaches them, so they
/// take precedence over a syntax error later in the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    ModeOff,
    TransformDisabled,
    BelowMinBytes,
    AboveMaxBytes,
    /// Bare scalar, invalid or truncated JSON, or trailing data, when no depth
    /// or key-policy reason occurs earlier in the input. On the
    /// `exact-line-runs` path, input that already starts with the `lines-v1`
    /// header, and on the `line-templates` path, input that already starts with
    /// the `lines-v1` or `lines-v2` header, so the codec never encodes its own
    /// output. Also table encoder (table-v1 or table-v2) shape ineligibility
    /// (see the eligibility tables in docs/compression-formats.md).
    UnsupportedFormat,
    DepthLimit,
    /// Consumers disagree on which duplicate wins. A duplicate in any object
    /// skips the whole document.
    DuplicateKey,
    /// An escaped key could hide a duplicate. An escaped key in any object
    /// skips the whole document.
    EscapedKey,
    /// The candidate is not shorter than the input, or, with a token counter,
    /// saves fewer tokens than the context thresholds require.
    InsufficientGain,
    InvalidCandidate,
    /// Set by the hook adapter: another `PostToolUse` hook may replace the same
    /// output, so the changed result is not emitted.
    HookConflict,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::ModeOff => "mode_off",
            Reason::TransformDisabled => "transform_disabled",
            Reason::BelowMinBytes => "below_min_bytes",
            Reason::AboveMaxBytes => "above_max_bytes",
            Reason::UnsupportedFormat => "unsupported_format",
            Reason::DepthLimit => "depth_limit",
            Reason::DuplicateKey => "duplicate_key",
            Reason::EscapedKey => "escaped_key",
            Reason::InsufficientGain => "insufficient_gain",
            Reason::InvalidCandidate => "invalid_candidate",
            Reason::HookConflict => "hook_conflict",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationOutcome {
    NotRun,
    Passed,
    Failed,
}

impl ValidationOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            ValidationOutcome::NotRun => "not_run",
            ValidationOutcome::Passed => "passed",
            ValidationOutcome::Failed => "failed",
        }
    }
}

/// Resource limits. Input sizes outside `min..=max` bytes are skipped.
/// Containers nested deeper than `max_json_depth` are skipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub min_candidate_bytes: usize,
    pub max_candidate_bytes: usize,
    /// Clamped to [`MAX_JSON_DEPTH_CEILING`], so no caller value can make the
    /// recursive scanner overflow the stack.
    pub max_json_depth: usize,
    /// Applies per encoded array: an array with more rows is never encoded by
    /// the table encoder (table-v1 or table-v2).
    pub max_table_rows: usize,
    /// A candidate must save at least this many tokens. Applies to every
    /// transform, only when the context has a token counter.
    pub min_estimated_token_savings: u64,
    /// A candidate must save at least this share of the original tokens, in
    /// permille: the policy ratio `min_estimated_savings_ratio = 0.10` is 100.
    /// Applies to every transform, only when the context has a token counter.
    pub min_estimated_savings_permille: u64,
}

/// Hard upper bound on [`Limits::max_json_depth`].
pub const MAX_JSON_DEPTH_CEILING: usize = 1024;

impl Default for Limits {
    /// Proposed starting values from the rollout policy.
    fn default() -> Self {
        Limits {
            min_candidate_bytes: 2048,
            max_candidate_bytes: 2_097_152,
            max_json_depth: 64,
            max_table_rows: 20_000,
            min_estimated_token_savings: 64,
            min_estimated_savings_permille: 100,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenMethod {
    /// Counted with the target model's own tokenizer.
    Exact,
    Estimate,
}

impl TokenMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            TokenMethod::Exact => "exact",
            TokenMethod::Estimate => "estimate",
        }
    }
}

/// A token count with its provenance. An unknown count is `None`, never zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenCount {
    pub count: u64,
    pub method: TokenMethod,
    /// Tokenizer name and version. Any string is accepted; an ASCII identifier
    /// is recommended. [`CompressionReport::to_json`] escapes it.
    pub tokenizer: &'static str,
}

/// Optional local token counting capability.
pub trait TokenCounter {
    fn count(&self, text: &str) -> TokenCount;
}

/// Everything [`compress`] may depend on. The same input and context yield the
/// same output.
#[derive(Clone, Copy)]
pub struct CompressionContext<'c> {
    pub mode: Mode,
    pub policy_version: u32,
    /// Enables the JSON minifier. Also the fallback when the table encoder
    /// does not apply.
    pub json_minify: bool,
    /// Enables the table encoder (table-v1 or table-v2), tried before the
    /// minifier. Opt-in: it is false in [`FIRST_SLICE_JSON_MINIFY_CONTEXT`].
    pub json_table: bool,
    /// Enables the exact line-run codec. With `json_minify` on, the codec
    /// handles text the JSON scanner rejects as `unsupported_format`. With
    /// `json_minify` off, input the table encoder rejects as
    /// `unsupported_format` (all input when `json_table` is off) goes to the
    /// codec. Other table encoder rejections skip with their reason and the
    /// `json-table` id. When `line_templates` is on, it takes precedence and
    /// the line-template codec takes this codec's place. The codec skips input
    /// that already starts with the `lines-v1` header as `unsupported_format`.
    /// `false` in [`FIRST_SLICE_JSON_MINIFY_CONTEXT`].
    pub exact_line_runs: bool,
    /// Enables the line-template codec. It takes the place of the line-run
    /// codec on the same routing and emits `lines-v2`, which also carries run
    /// directives. The codec skips input that already starts with the `lines-v1`
    /// or `lines-v2` header as `unsupported_format`. `false` in
    /// [`FIRST_SLICE_JSON_MINIFY_CONTEXT`].
    pub line_templates: bool,
    pub limits: Limits,
    /// Without a counter, every token field in the report is `None`.
    pub token_counter: Option<&'c dyn TokenCounter>,
}

/// Safe mode, JSON minifier on, no size limits and a depth limit of 128, so
/// adapters keep the behavior of the first minification slice.
pub const FIRST_SLICE_JSON_MINIFY_CONTEXT: CompressionContext<'static> = CompressionContext {
    mode: Mode::Safe,
    policy_version: 1,
    json_minify: true,
    json_table: false,
    exact_line_runs: false,
    line_templates: false,
    limits: Limits {
        min_candidate_bytes: 0,
        max_candidate_bytes: usize::MAX,
        max_json_depth: 128,
        max_table_rows: 20_000,
        min_estimated_token_savings: 0,
        min_estimated_savings_permille: 0,
    },
    token_counter: None,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompressionReport {
    pub schema_version: u32,
    pub transform_id: &'static str,
    pub transform_version: u32,
    /// `None` when the output is plain JSON with no deslag-defined format.
    pub format_version: Option<u32>,
    pub policy_version: u32,
    pub mode: Mode,
    pub outcome: Outcome,
    pub reason: Option<Reason>,
    pub original_bytes: usize,
    /// `None` when no candidate was built.
    pub candidate_bytes: Option<usize>,
    pub emitted_bytes: usize,
    pub original_tokens: Option<TokenCount>,
    pub candidate_tokens: Option<TokenCount>,
    pub emitted_tokens: Option<TokenCount>,
    /// Wall time of the whole [`compress`] call, including token counting.
    pub compression_duration: Duration,
    pub validation_outcome: ValidationOutcome,
}

impl CompressionReport {
    /// One JSON object with snake_case keys. `reason` is serialized as
    /// `skip_or_failure_reason`, `compression_duration` as whole nanoseconds,
    /// and every absent value as `null`.
    pub fn to_json(&self) -> String {
        fn opt<T>(v: Option<T>, f: impl FnOnce(T) -> String) -> String {
            v.map_or_else(|| "null".to_string(), f)
        }
        // Every string value goes through `q`, so a caller-supplied tokenizer
        // name cannot break the JSON.
        let q = json_string;
        let tokens = |t: Option<TokenCount>| {
            opt(t, |t| {
                format!(
                    r#"{{"count":{},"method":{},"tokenizer":{}}}"#,
                    t.count,
                    q(t.method.as_str()),
                    q(t.tokenizer)
                )
            })
        };
        let fields = [
            ("schema_version", self.schema_version.to_string()),
            ("transform_id", q(self.transform_id)),
            ("transform_version", self.transform_version.to_string()),
            (
                "format_version",
                opt(self.format_version, |v| v.to_string()),
            ),
            ("policy_version", self.policy_version.to_string()),
            ("mode", q(self.mode.as_str())),
            ("outcome", q(self.outcome.as_str())),
            (
                "skip_or_failure_reason",
                opt(self.reason, |r| q(r.as_str())),
            ),
            ("original_bytes", self.original_bytes.to_string()),
            (
                "candidate_bytes",
                opt(self.candidate_bytes, |v| v.to_string()),
            ),
            ("emitted_bytes", self.emitted_bytes.to_string()),
            ("original_tokens", tokens(self.original_tokens)),
            ("candidate_tokens", tokens(self.candidate_tokens)),
            ("emitted_tokens", tokens(self.emitted_tokens)),
            (
                "compression_duration",
                self.compression_duration.as_nanos().to_string(),
            ),
            ("validation_outcome", q(self.validation_outcome.as_str())),
        ];
        let mut out = String::from("{");
        for (i, (key, value)) in fields.iter().enumerate() {
            let sep = if i == 0 { "" } else { "," };
            let _ = write!(out, "{sep}\"{key}\":{value}");
        }
        out.push('}');
        out
    }
}

/// Quotes `s` as a JSON string: escapes `"`, `\` and control characters.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c < ' ' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Debug)]
pub struct CompressionResult<'a> {
    /// Borrows `input` for every outcome except `Changed`.
    pub output: Cow<'a, str>,
    pub report: CompressionReport,
}

impl CompressionResult<'_> {
    /// The rewritten content when the outcome is `Changed`, else `None`.
    /// Adapters commit only this value; every other outcome keeps the original.
    pub fn into_changed(self) -> Option<String> {
        (self.report.outcome == Outcome::Changed).then(|| self.output.into_owned())
    }
}

/// Runs the enabled transforms under `ctx`: the table encoder (table-v1 or
/// table-v2) first, then the JSON minifier, then the exact line-run codec for
/// input the JSON transforms reject as unsupported_format. Any skip or failure
/// returns the input borrowed, never a partial rewrite.
pub fn compress<'a>(input: &'a str, ctx: &CompressionContext) -> CompressionResult<'a> {
    let start = Instant::now();
    let mut report = CompressionReport {
        schema_version: REPORT_SCHEMA_VERSION,
        transform_id: JSON_MINIFY_ID,
        transform_version: JSON_MINIFY_VERSION,
        format_version: None,
        policy_version: ctx.policy_version,
        mode: ctx.mode,
        outcome: Outcome::Skipped,
        reason: None,
        original_bytes: input.len(),
        candidate_bytes: None,
        emitted_bytes: input.len(),
        original_tokens: None,
        candidate_tokens: None,
        emitted_tokens: None,
        compression_duration: Duration::ZERO,
        validation_outcome: ValidationOutcome::NotRun,
    };
    let output = match decide(input, ctx, &mut report) {
        Ok(candidate) => candidate,
        Err(reason) => {
            report.reason = Some(reason);
            if reason == Reason::InvalidCandidate {
                report.outcome = Outcome::Failed;
            }
            Cow::Borrowed(input)
        }
    };
    report.compression_duration = start.elapsed();
    CompressionResult { output, report }
}

/// Fills candidate fields in `report`. Returns the output for a valid
/// candidate with gain, or the reason the input passes through.
fn decide<'a>(
    input: &'a str,
    ctx: &CompressionContext,
    report: &mut CompressionReport,
) -> Result<Cow<'a, str>, Reason> {
    let limits = &ctx.limits;
    // A table-only context reports json-table for every outcome, early skips included.
    if ctx.json_table && !ctx.json_minify {
        report.transform_id = JSON_TABLE_ID;
        report.transform_version = JSON_TABLE_VERSION;
    }
    if ctx.mode == Mode::Off {
        return Err(Reason::ModeOff);
    }
    let lines = ctx.exact_line_runs || ctx.line_templates;
    if !ctx.json_minify && !ctx.json_table && !lines {
        return Err(Reason::TransformDisabled);
    }
    if !ctx.json_minify && !ctx.json_table {
        set_line_runs_identity(report, ctx);
    }
    if input.len() < limits.min_candidate_bytes {
        return Err(Reason::BelowMinBytes);
    }
    if input.len() > limits.max_candidate_bytes {
        return Err(Reason::AboveMaxBytes);
    }
    let json = if ctx.json_table {
        let opts = TableOptions {
            max_rows: limits.max_table_rows,
            max_depth: limits.max_json_depth,
            token_counter: ctx.token_counter,
            ..TableOptions::default()
        };
        match table::v2::encode_minified(input, &opts) {
            Ok((text, minified)) => {
                report.transform_id = JSON_TABLE_ID;
                report.transform_version = JSON_TABLE_VERSION;
                report.format_version = Some(if text.starts_with(table::v2::MARKER) {
                    JSON_TABLE_V2_FORMAT_VERSION
                } else {
                    JSON_TABLE_FORMAT_VERSION
                });
                report.candidate_bytes = Some(text.len());
                if table::v2::decode(&text) != Some(minified) {
                    report.validation_outcome = ValidationOutcome::Failed;
                    return Err(Reason::InvalidCandidate);
                }
                return emit(input, text, ctx, report);
            }
            Err(_) if ctx.json_minify => minify(input, limits.max_json_depth),
            Err(reason) => Err(reason),
        }
    } else if ctx.json_minify {
        minify(input, limits.max_json_depth)
    } else {
        Err(Reason::UnsupportedFormat)
    };
    let candidate = match json {
        Ok(bytes) => {
            report.candidate_bytes = Some(bytes.len());
            // Output is ASCII structure plus whole string lexemes from valid UTF-8
            // input, so this check guards the scanner itself.
            let Ok(candidate) = String::from_utf8(bytes) else {
                report.validation_outcome = ValidationOutcome::Failed;
                return Err(Reason::InvalidCandidate);
            };
            candidate
        }
        // Text the JSON transforms reject as unsupported_format is plain text. A
        // leading value rejected for duplicate_key, escaped_key or depth_limit
        // keeps the input on the JSON path, even when text lines follow. With
        // json_minify off, input the table encoder (table-v1 or table-v2) rejects
        // as unsupported_format (all input when json_table is off) arrives here.
        // Other table encoder rejections (insufficient_gain, duplicate_key,
        // escaped_key, depth_limit) skip with that reason and the json-table id.
        Err(Reason::UnsupportedFormat) if lines => {
            set_line_runs_identity(report, ctx);
            // Input that already starts with a header the codec emits is skipped as
            // unsupported_format, so the codec never encodes its own output.
            let candidate = if ctx.line_templates {
                encode_templates(input)
            } else {
                encode_lines(input)
            }
            .ok_or(Reason::UnsupportedFormat)?;
            report.candidate_bytes = Some(candidate.len());
            if candidate.len() >= input.len() {
                return Err(Reason::InsufficientGain);
            }
            if decode_lines(&candidate, input.len()).as_deref() != Ok(input) {
                report.validation_outcome = ValidationOutcome::Failed;
                return Err(Reason::InvalidCandidate);
            }
            candidate
        }
        Err(reason) => return Err(reason),
    };
    emit(input, candidate, ctx, report)
}

/// Records a validated candidate and returns the output for the mode.
fn emit<'a>(
    input: &'a str,
    candidate: String,
    ctx: &CompressionContext,
    report: &mut CompressionReport,
) -> Result<Cow<'a, str>, Reason> {
    report.validation_outcome = ValidationOutcome::Passed;
    if let Some(counter) = ctx.token_counter {
        report.original_tokens = Some(counter.count(input));
        report.candidate_tokens = Some(counter.count(&candidate));
        report.emitted_tokens = report.original_tokens;
    }
    if candidate.len() >= input.len() {
        return Err(Reason::InsufficientGain);
    }
    if let (Some(original), Some(cand)) = (report.original_tokens, report.candidate_tokens) {
        let limits = &ctx.limits;
        // A token regression fails even with both thresholds at zero.
        let enough = original.count.checked_sub(cand.count).is_some_and(|saved| {
            saved >= limits.min_estimated_token_savings
                && u128::from(saved) * 1000
                    >= u128::from(limits.min_estimated_savings_permille)
                        * u128::from(original.count)
        });
        if !enough {
            return Err(Reason::InsufficientGain);
        }
    }
    if ctx.mode == Mode::Observe {
        report.outcome = Outcome::WouldChange;
        return Ok(Cow::Borrowed(input));
    }
    report.outcome = Outcome::Changed;
    report.emitted_bytes = candidate.len();
    report.emitted_tokens = report.candidate_tokens;
    Ok(Cow::Owned(candidate))
}

fn set_line_runs_identity(report: &mut CompressionReport, ctx: &CompressionContext) {
    if ctx.line_templates {
        report.transform_id = LINE_TEMPLATES_ID;
        report.transform_version = LINE_TEMPLATES_VERSION;
        report.format_version = Some(LINES_V2_FORMAT_VERSION);
    } else {
        report.transform_id = EXACT_LINE_RUNS_ID;
        report.transform_version = EXACT_LINE_RUNS_VERSION;
        report.format_version = Some(LINES_FORMAT_VERSION);
    }
}

/// Removes insignificant whitespace from a JSON object or array.
///
/// Every string, number and literal lexeme is copied byte for byte, so values,
/// escapes and number precision are preserved. Member and element order is kept.
///
/// Ambiguous input is skipped as a whole: one duplicate or escaped key anywhere,
/// however deep, makes the entire document pass through, because a partial
/// rewrite of an ambiguous document is never emitted. Escaped string values and
/// keys repeated across sibling objects are not ambiguous and still minify.
///
/// `None` means: pass the input through unchanged. Equivalent to [`compress`]
/// with [`FIRST_SLICE_JSON_MINIFY_CONTEXT`]; `Some` exactly when the outcome
/// is `Changed`.
pub fn minify_json(input: &str) -> Option<String> {
    compress(input, &FIRST_SLICE_JSON_MINIFY_CONTEXT).into_changed()
}

/// Scans `input` and returns its minified bytes, with no gain check.
fn minify(input: &str, max_depth: usize) -> Result<Vec<u8>, Reason> {
    #[cfg(test)]
    tests::MINIFIED.with_borrow_mut(|v| v.push(input.as_ptr()));
    let bytes = input.as_bytes();
    let mut s = Scanner {
        b: bytes,
        i: 0,
        out: Vec::with_capacity(bytes.len()),
        max_depth: max_depth.min(MAX_JSON_DEPTH_CEILING),
    };
    s.ws();
    // Bare scalars are treated as plain text.
    if !matches!(s.peek(), Some(b'{' | b'[')) {
        return Err(Reason::UnsupportedFormat);
    }
    s.value(0)?;
    s.ws();
    if s.i != bytes.len() {
        return Err(Reason::UnsupportedFormat);
    }
    Ok(s.out)
}

type Scan<T = ()> = Result<T, Reason>;

const BAD: Reason = Reason::UnsupportedFormat;

struct Scanner<'a> {
    b: &'a [u8],
    i: usize,
    out: Vec<u8>,
    max_depth: usize,
}

impl<'a> Scanner<'a> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn next(&self) -> Scan<u8> {
        self.peek().ok_or(BAD)
    }

    /// JSON whitespace only; any other byte outside a lexeme is invalid.
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        let hit = self.peek() == Some(c);
        if hit {
            self.out.push(c);
            self.i += 1;
        }
        hit
    }

    fn expect(&mut self, c: u8) -> Scan {
        if self.eat(c) { Ok(()) } else { Err(BAD) }
    }

    fn value(&mut self, depth: usize) -> Scan {
        match self.next()? {
            b'{' => self.object(depth + 1),
            b'[' => self.array(depth + 1),
            b'"' => self.string().map(drop),
            b't' => self.literal(b"true"),
            b'f' => self.literal(b"false"),
            b'n' => self.literal(b"null"),
            _ => self.number(),
        }
    }

    fn object(&mut self, depth: usize) -> Scan {
        if depth > self.max_depth {
            return Err(Reason::DepthLimit);
        }
        self.expect(b'{')?;
        self.ws();
        if self.eat(b'}') {
            return Ok(());
        }
        let mut keys = HashSet::new();
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(BAD);
            }
            let key = self.string()?;
            // ponytail: keys are compared as raw lexemes, and any escaped key skips
            // the whole document. An escaped key such as `"\u0061"` decodes to the
            // same key as `"a"`, so the two could hide a duplicate. Ceiling: one such
            // key anywhere forfeits all savings for the document. Decode keys if
            // escaped keys turn out to be common in real tool output.
            if key.contains(&b'\\') {
                return Err(Reason::EscapedKey);
            }
            if !keys.insert(key) {
                return Err(Reason::DuplicateKey);
            }
            self.ws();
            self.expect(b':')?;
            self.ws();
            self.value(depth)?;
            self.ws();
            if !self.eat(b',') {
                return self.expect(b'}');
            }
        }
    }

    fn array(&mut self, depth: usize) -> Scan {
        if depth > self.max_depth {
            return Err(Reason::DepthLimit);
        }
        self.expect(b'[')?;
        self.ws();
        if self.eat(b']') {
            return Ok(());
        }
        loop {
            self.ws();
            self.value(depth)?;
            self.ws();
            if !self.eat(b',') {
                return self.expect(b']');
            }
        }
    }

    /// Validates one string lexeme, copies it verbatim and returns it with its quotes.
    fn string(&mut self) -> Scan<&'a [u8]> {
        let start = self.i;
        self.i += 1;
        loop {
            match self.next()? {
                b'"' => break,
                b'\\' => {
                    self.i += 1;
                    match self.next()? {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => self.i += 1,
                        b'u' => {
                            let hex = self.b.get(self.i + 1..self.i + 5).ok_or(BAD)?;
                            if !hex.iter().all(u8::is_ascii_hexdigit) {
                                return Err(BAD);
                            }
                            self.i += 5;
                        }
                        _ => return Err(BAD),
                    }
                }
                c if c < 0x20 => return Err(BAD),
                _ => self.i += 1,
            }
        }
        self.i += 1;
        let lexeme = &self.b[start..self.i];
        self.out.extend_from_slice(lexeme);
        Ok(lexeme)
    }

    fn literal(&mut self, word: &[u8]) -> Scan {
        if !self.b[self.i..].starts_with(word) {
            return Err(BAD);
        }
        self.out.extend_from_slice(word);
        self.i += word.len();
        Ok(())
    }

    /// RFC 8259 number grammar, copied verbatim.
    fn number(&mut self) -> Scan {
        let start = self.i;
        self.skip(b'-');
        match self.next()? {
            b'0' => self.i += 1,
            b'1'..=b'9' => self.digits(),
            _ => return Err(BAD),
        }
        if self.skip(b'.') {
            self.require_digits()?;
        }
        if self.skip(b'e') || self.skip(b'E') {
            let _ = self.skip(b'+') || self.skip(b'-');
            self.require_digits()?;
        }
        self.out.extend_from_slice(&self.b[start..self.i]);
        Ok(())
    }

    fn skip(&mut self, c: u8) -> bool {
        let hit = self.peek() == Some(c);
        self.i += usize::from(hit);
        hit
    }

    fn digits(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
    }

    fn require_digits(&mut self) -> Scan {
        let start = self.i;
        self.digits();
        if self.i > start { Ok(()) } else { Err(BAD) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        /// The address of each text passed to `minify` on this thread.
        pub(super) static MINIFIED: std::cell::RefCell<Vec<*const u8>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    #[test]
    fn version_is_set() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn table_path_minifies_the_input_once() {
        let rows = |n: usize| -> String {
            let items: Vec<String> = (0..n)
                .map(|i| format!(r#"{{"id":{i},"name":"user-{i}","on":true}}"#))
                .collect();
            format!("[{}]", items.join(",\n"))
        };
        let nested = format!(
            r#"[{{"t":"a","j":"{0}"}},{{"t":"b","j":"{0}"}}]"#,
            rows(6).replace('\n', "").replace('"', "\\\"")
        );
        let ctx = CompressionContext {
            json_minify: false,
            json_table: true,
            ..FIRST_SLICE_JSON_MINIFY_CONTEXT
        };
        // table-v1, table-v2 with sub-blocks, and a wrapper object.
        let cases = [
            (rows(40), "table-v1 ["),
            (nested, "table-v2 @0\n@0 "),
            (format!(r#"{{"rows":{}}}"#, rows(40)), "table-v2 {"),
        ];
        for (input, start) in cases {
            MINIFIED.with_borrow_mut(Vec::clear);
            let result = compress(&input, &ctx);
            assert_eq!(result.report.outcome, Outcome::Changed, "{input}");
            assert!(result.output.starts_with(start), "{}", result.output);
            let passes =
                MINIFIED.with_borrow(|v| v.iter().filter(|&&p| p == input.as_ptr()).count());
            assert_eq!(passes, 1, "{input}");
        }
    }
}
