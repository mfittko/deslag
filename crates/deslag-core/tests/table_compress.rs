use std::borrow::Cow;

use deslag_core::{
    CompressionContext, CompressionResult, FIRST_SLICE_JSON_MINIFY_CONTEXT, JSON_MINIFY_ID,
    JSON_MINIFY_VERSION, JSON_TABLE_FORMAT_VERSION, JSON_TABLE_ID, JSON_TABLE_VERSION, Limits,
    Mode, Outcome, Reason, TableOptions, TokenCount, TokenCounter, TokenMethod, ValidationOutcome,
    compress, minify_json,
    table::{MARKER, decode, encode, render_markdown_kv},
};

struct Bytes;

impl TokenCounter for Bytes {
    fn count(&self, text: &str) -> TokenCount {
        TokenCount {
            count: text.len() as u64,
            method: TokenMethod::Estimate,
            tokenizer: "bytes",
        }
    }
}

/// `n` pretty-printed uniform rows: eligible for table-v1 with ample gain.
fn rows(n: usize) -> String {
    let items: Vec<String> = (0..n)
        .map(|i| format!("  {{ \"id\": {i}, \"name\": \"user-{i}\", \"active\": true }}"))
        .collect();
    format!("[\n{}\n]", items.join(",\n"))
}

/// Pretty object: the minifier applies, table-v1 does not.
const OBJECT: &str = "{ \"a\": 1,  \"b\": [1, 2] }";

fn ctx(mode: Mode, json_minify: bool, json_table: bool) -> CompressionContext<'static> {
    CompressionContext {
        mode,
        json_minify,
        json_table,
        ..FIRST_SLICE_JSON_MINIFY_CONTEXT
    }
}

fn borrowed(result: &CompressionResult<'_>, input: &str) -> bool {
    matches!(&result.output, Cow::Borrowed(s) if std::ptr::eq(*s, input))
}

#[test]
fn table_is_off_by_default() {
    const { assert!(!FIRST_SLICE_JSON_MINIFY_CONTEXT.json_table) };
    assert_eq!(Limits::default().max_table_rows, 20_000);
    assert_eq!(
        FIRST_SLICE_JSON_MINIFY_CONTEXT.limits.max_table_rows,
        20_000
    );
    let input = rows(10);
    let result = compress(&input, &FIRST_SLICE_JSON_MINIFY_CONTEXT);
    assert_eq!(result.report.outcome, Outcome::Changed);
    assert_eq!(result.report.transform_id, JSON_MINIFY_ID);
    assert_eq!(result.report.format_version, None);
    assert!(!result.output.starts_with(MARKER));
    assert!(!minify_json(&input).unwrap().starts_with(MARKER));
}

#[test]
fn table_on_emits_validated_table_v1() {
    let input = rows(10);
    let counter = Bytes;
    let ctx = CompressionContext {
        token_counter: Some(&counter),
        ..ctx(Mode::Safe, true, true)
    };
    let result = compress(&input, &ctx);
    let r = &result.report;
    assert_eq!(r.outcome, Outcome::Changed);
    assert_eq!(r.reason, None);
    assert_eq!(r.transform_id, JSON_TABLE_ID);
    assert_eq!(r.transform_version, JSON_TABLE_VERSION);
    assert_eq!(r.format_version, Some(JSON_TABLE_FORMAT_VERSION));
    assert_eq!(r.validation_outcome, ValidationOutcome::Passed);
    assert_eq!(r.original_bytes, input.len());
    assert_eq!(r.candidate_bytes, Some(result.output.len()));
    assert_eq!(r.emitted_bytes, result.output.len());
    assert_eq!(r.original_tokens, Some(counter.count(&input)));
    assert_eq!(r.candidate_tokens, Some(counter.count(&result.output)));
    assert_eq!(r.emitted_tokens, r.candidate_tokens);
    assert!(
        result
            .output
            .starts_with("table-v1 [10]{id:int,name:str,active:bool}\n")
    );
    assert_eq!(decode(&result.output), minify_json(&input));
    assert!(r.to_json().contains(r#""transform_id":"json-table""#));
    assert!(r.to_json().contains(r#""format_version":1"#));
}

#[test]
fn observe_emits_original_with_would_change() {
    let input = rows(10);
    let result = compress(&input, &ctx(Mode::Observe, true, true));
    assert!(borrowed(&result, &input));
    let r = &result.report;
    assert_eq!(r.outcome, Outcome::WouldChange);
    assert_eq!(r.transform_id, JSON_TABLE_ID);
    assert_eq!(r.emitted_bytes, input.len());
    assert!(r.candidate_bytes.unwrap() < input.len());
}

#[test]
fn ineligible_input_falls_back_to_minify() {
    let result = compress(OBJECT, &ctx(Mode::Safe, true, true));
    let r = &result.report;
    assert_eq!(r.outcome, Outcome::Changed);
    assert_eq!(r.transform_id, JSON_MINIFY_ID);
    assert_eq!(r.transform_version, JSON_MINIFY_VERSION);
    assert_eq!(r.format_version, None);
    assert_eq!(result.output, minify_json(OBJECT).unwrap());
}

#[test]
fn max_table_rows_is_enforced() {
    let input = rows(10);
    let mut ctx = ctx(Mode::Safe, true, true);
    ctx.limits.max_table_rows = 10;
    assert_eq!(compress(&input, &ctx).report.transform_id, JSON_TABLE_ID);
    ctx.limits.max_table_rows = 9;
    let result = compress(&input, &ctx);
    assert_eq!(result.report.transform_id, JSON_MINIFY_ID);
    assert_eq!(result.report.outcome, Outcome::Changed);
    ctx.json_minify = false;
    let result = compress(&input, &ctx);
    assert_eq!(result.report.reason, Some(Reason::UnsupportedFormat));
    assert!(borrowed(&result, &input));
}

#[test]
fn table_only_reports_the_table_reason() {
    let table_only = ctx(Mode::Safe, false, true);
    let result = compress(OBJECT, &table_only);
    let r = &result.report;
    assert!(borrowed(&result, OBJECT));
    assert_eq!(r.outcome, Outcome::Skipped);
    assert_eq!(r.reason, Some(Reason::UnsupportedFormat));
    assert_eq!(r.transform_id, JSON_TABLE_ID);
    assert_eq!(r.candidate_bytes, None);
    let short = r#"[{"a":1},{"a":2}]"#;
    assert_eq!(
        compress(short, &table_only).report.reason,
        Some(Reason::InsufficientGain)
    );
    assert_eq!(
        compress(&rows(10), &ctx(Mode::Safe, false, false))
            .report
            .reason,
        Some(Reason::TransformDisabled)
    );
}

fn lenient() -> TableOptions<'static> {
    TableOptions {
        min_savings_permille: 0,
        ..TableOptions::default()
    }
}

#[test]
fn markdown_kv_renders_rows_as_list_items() {
    let input = r#"[
      {"id": 1, "name": "a", "note": null, "meta": {"region": "eu"}},
      {"id": 2, "name": "", "meta": {"region": "null"}},
      {"id": 3, "name": "1.5", "note": "x, \"y\"", "meta": {"region": "true"}}
    ]"#;
    let kv = render_markdown_kv(input, &lenient()).unwrap();
    assert_eq!(
        kv,
        "table-v1 [3]{id:int,name:str,note:str,meta.region:str}\n\
         - id: 1\n  name: a\n  note: null\n  meta.region: eu\n\
         - id: 2\n  name: \"\"\n  meta.region: \"null\"\n\
         - id: 3\n  name: \"1.5\"\n  note: \"x, \"\"y\"\"\"\n  meta.region: \"true\""
    );
    let csv = encode(input, &lenient()).unwrap();
    assert_eq!(kv.lines().next(), csv.lines().next());
}

#[test]
fn markdown_kv_renders_an_empty_row_as_a_bare_dash() {
    let input = r#"[{"a_long_key_name":1},{},{"a_long_key_name":2}]"#;
    assert_eq!(
        render_markdown_kv(input, &lenient()).unwrap(),
        "table-v1 [3]{a_long_key_name:int}\n- a_long_key_name: 1\n-\n- a_long_key_name: 2"
    );
}

#[test]
fn markdown_kv_follows_encoder_eligibility() {
    for input in [OBJECT, r#"[{"a":1}]"#, r#"[{"a":[1]},{"a":[2]}]"#] {
        assert_eq!(render_markdown_kv(input, &lenient()), None, "{input}");
    }
    // The gain check applies to the CSV form, as for `encode`.
    let short = r#"[{"a":1},{"a":2}]"#;
    assert!(encode(short, &TableOptions::default()).is_err());
    assert_eq!(render_markdown_kv(short, &TableOptions::default()), None);
}

#[test]
fn compress_never_selects_markdown_kv() {
    let input = rows(10);
    let kv = render_markdown_kv(&input, &TableOptions::default()).unwrap();
    for mode in [Mode::Off, Mode::Observe, Mode::Safe] {
        for json_minify in [false, true] {
            for json_table in [false, true] {
                let ctx = ctx(mode, json_minify, json_table);
                let result = compress(&input, &ctx);
                assert_ne!(result.output, kv);
                assert!(
                    !result.output.lines().any(|l| l.starts_with("- ")),
                    "{mode:?} {json_minify} {json_table}"
                );
            }
        }
    }
}
