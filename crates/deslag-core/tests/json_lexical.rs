use std::borrow::Cow;

use deslag_core::{
    CompressionContext, FIRST_SLICE_JSON_MINIFY_CONTEXT, Mode, Outcome, Reason, ValidationOutcome,
    compress, minify_json,
};

/// Asserts that `input` passes through borrowed with `reason` and no candidate.
fn assert_passthrough(input: &str, ctx: &CompressionContext, reason: Reason) {
    let result = compress(input, ctx);
    assert!(
        matches!(&result.output, Cow::Borrowed(s) if std::ptr::eq(*s, input)),
        "not borrowed: {input:?}"
    );
    assert_eq!(result.report.outcome, Outcome::Skipped, "{input:?}");
    assert_eq!(result.report.reason, Some(reason), "{input:?}");
    assert_eq!(result.report.candidate_bytes, None, "{input:?}");
    assert_eq!(
        result.report.validation_outcome,
        ValidationOutcome::NotRun,
        "{input:?}"
    );
}

fn assert_minifies(input: &str, expected: &str) {
    assert_eq!(minify_json(input).as_deref(), Some(expected), "{input:?}");
}

#[test]
fn negative_zero_keeps_its_lexeme() {
    assert_minifies(
        "[ -0 , -0.0 , 0.0 , -0e0 , -0.000E-0 ]",
        "[-0,-0.0,0.0,-0e0,-0.000E-0]",
    );
}

#[test]
fn exponent_forms_keep_their_lexeme() {
    assert_minifies(
        "[ 1e5, 1E5, 1e+5, 1E+5, 1e-5, 1E-5, 1e05, 0e0, -2.50E-300, 1e400 ]",
        "[1e5,1E5,1e+5,1E+5,1e-5,1E-5,1e05,0e0,-2.50E-300,1e400]",
    );
}

#[test]
fn integers_beyond_u64_and_f64_precision_keep_every_digit() {
    // 2^64, below i64::MIN, 2^53 + 1, and a decimal far beyond f64 precision.
    assert_minifies(
        "{\n  \"u\": 18446744073709551616,\n  \"i\": -9223372036854775809,\n  \"f\": 9007199254740993,\n  \"d\": 123456789012345678901234567890.000000000000000000001\n}",
        r#"{"u":18446744073709551616,"i":-9223372036854775809,"f":9007199254740993,"d":123456789012345678901234567890.000000000000000000001}"#,
    );
}

#[test]
fn escaped_unicode_is_copied_verbatim() {
    // BMP, surrogate pair in both cases, lone high and low surrogates, and NUL.
    let value = r#""\u00e9 \u00E9 \uD83E\uDD80 \ud83e\udd80 \ud800 \udc00 \uDBFF\uDFFF \u0000""#;
    assert_minifies(
        &format!("{{ \"s\" : {value} }}"),
        &format!("{{\"s\":{value}}}"),
    );
}

#[test]
fn every_escape_and_whitespace_inside_strings_is_kept() {
    let value =
        "\"\\\" \\\\ \\/ \\b \\f \\n \\r \\t  a : b , [ ] { }  \u{a0}\u{2028}\u{3000} 日本語 🦀 \"";
    assert_minifies(
        &format!("[\n\t{value} ,\n\t{value}\n]"),
        &format!("[{value},{value}]"),
    );
}

const FIXTURE: &str = r#"
{
  "id": -0,
  "exp": [ 1E+10, 2.5e-3 ],
  "big": 12345678901234567890123,
  "text": "a \"q\" \\ \/ \b\f\n\r\t \u00e9 \uD83E\uDD80 日本語",
  "flags": [ true, false, null ],
  "nested": { "list": [ { }, [ ] ] }
}
"#;

#[test]
fn every_strict_prefix_passes_through() {
    let ctx = FIRST_SLICE_JSON_MINIFY_CONTEXT;
    let text = FIXTURE.trim();
    assert!(minify_json(text).is_some());
    let cuts = (0..text.len()).filter(|&i| text.is_char_boundary(i));
    for cut in cuts {
        assert_passthrough(&text[..cut], &ctx, Reason::UnsupportedFormat);
    }
}

#[test]
fn bom_prefixed_input_passes_through() {
    let input = format!("\u{feff}{FIXTURE}");
    assert_passthrough(
        &input,
        &FIRST_SLICE_JSON_MINIFY_CONTEXT,
        Reason::UnsupportedFormat,
    );
}

/// A large pretty document. Every item repeats the keys of its siblings and
/// carries escaped string values. Item 150 holds `inner` deep inside.
fn large_document(inner: &str) -> String {
    let mut out = String::from("{\n  \"items\": [\n");
    for i in 0..200 {
        let sep = if i == 0 { "" } else { ",\n" };
        let meta = if i == 150 {
            format!("{{ \"a\": {{ \"b\": [ {{ {inner} }} ] }} }}")
        } else {
            "{ }".to_string()
        };
        out.push_str(&format!(
            "{sep}    {{ \"id\": {i}, \"name\": \"item \\\"{i}\\\" \\u00e9\\n\", \"meta\": {meta} }}"
        ));
    }
    out.push_str("\n  ]\n}\n");
    out
}

#[test]
fn key_policy_skips_the_whole_document() {
    let ctx = FIRST_SLICE_JSON_MINIFY_CONTEXT;
    let control = large_document("\"k\": 1, \"j\": 2");
    let out = minify_json(&control).expect("control minifies");
    let a: serde_json::Value = serde_json::from_str(&control).unwrap();
    let b: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(a, b);

    let duplicate = large_document("\"k\": 1, \"k\": 2");
    assert_passthrough(&duplicate, &ctx, Reason::DuplicateKey);
    let escaped = large_document("\"k\": 1, \"\\u006a\": 2");
    assert_passthrough(&escaped, &ctx, Reason::EscapedKey);
}

#[test]
fn policy_bypass_returns_exact_bytes() {
    // Aligned columns a byte-sensitive consumer may depend on.
    let input = "{\n  \"name\"  : \"a\",\n  \"value\" : 1\n}\n";
    let disabled = CompressionContext {
        json_minify: false,
        ..FIRST_SLICE_JSON_MINIFY_CONTEXT
    };
    let off = CompressionContext {
        mode: Mode::Off,
        ..FIRST_SLICE_JSON_MINIFY_CONTEXT
    };
    for (ctx, reason) in [
        (disabled, Reason::TransformDisabled),
        (off, Reason::ModeOff),
    ] {
        assert_passthrough(input, &ctx, reason);
    }
}
