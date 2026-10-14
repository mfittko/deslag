use std::{borrow::Cow, time::Duration};

use deslag_core::{
    CompressionContext, CompressionReport, CompressionResult, Limits, Mode, Outcome, Reason,
    TokenCount, TokenCounter, TokenMethod, ValidationOutcome, compress,
};

const PRETTY: &str = "{\n  \"a\": [1, 2, 3],\n  \"b\": \"x y\"\n}\n";

const FIELDS: [&str; 16] = [
    "schema_version",
    "transform_id",
    "transform_version",
    "format_version",
    "policy_version",
    "mode",
    "outcome",
    "skip_or_failure_reason",
    "original_bytes",
    "candidate_bytes",
    "emitted_bytes",
    "original_tokens",
    "candidate_tokens",
    "emitted_tokens",
    "compression_duration",
    "validation_outcome",
];

/// Counts bytes as tokens.
struct ByteCounter;

impl TokenCounter for ByteCounter {
    fn count(&self, text: &str) -> TokenCount {
        TokenCount {
            count: text.len() as u64,
            method: TokenMethod::Estimate,
            tokenizer: "test-bytes",
        }
    }
}

fn ctx(mode: Mode) -> CompressionContext<'static> {
    CompressionContext {
        mode,
        policy_version: 7,
        json_minify: true,
        json_table: false,
        exact_line_runs: false,
        line_templates: false,
        limits: Limits {
            min_candidate_bytes: 0,
            min_estimated_token_savings: 0,
            min_estimated_savings_permille: 0,
            ..Limits::default()
        },
        token_counter: None,
    }
}

fn is_borrowed_from(result: &CompressionResult<'_>, input: &str) -> bool {
    matches!(&result.output, Cow::Borrowed(s) if std::ptr::eq(*s, input))
}

fn report_json(result: &CompressionResult<'_>) -> serde_json::Map<String, serde_json::Value> {
    report_map(&result.report)
}

fn report_map(report: &CompressionReport) -> serde_json::Map<String, serde_json::Value> {
    match serde_json::from_str(&report.to_json()).unwrap() {
        serde_json::Value::Object(map) => map,
        other => panic!("report is not an object: {other}"),
    }
}

#[test]
fn defaults_match_plan() {
    let limits = Limits::default();
    assert_eq!(limits.min_candidate_bytes, 2048);
    assert_eq!(limits.max_candidate_bytes, 2_097_152);
    assert_eq!(limits.max_json_depth, 64);
    assert_eq!(limits.min_estimated_token_savings, 64);
    assert_eq!(limits.min_estimated_savings_permille, 100);
}

#[test]
fn report_schema_has_every_field_for_each_outcome() {
    let cases = [
        (ctx(Mode::Safe), PRETTY, ("changed", "safe", "passed")),
        (
            ctx(Mode::Observe),
            PRETTY,
            ("would_change", "observe", "passed"),
        ),
        (ctx(Mode::Off), PRETTY, ("skipped", "off", "not_run")),
        (ctx(Mode::Safe), "[1,2]", ("skipped", "safe", "passed")),
    ];
    let mut reports: Vec<(CompressionReport, (&str, &str, &str))> = cases
        .into_iter()
        .map(|(ctx, input, want)| (compress(input, &ctx).report, want))
        .collect();
    // No input reaches `failed`, so that report is built directly.
    reports.push((
        CompressionReport {
            schema_version: 1,
            transform_id: "json-minify",
            transform_version: 1,
            format_version: None,
            policy_version: 7,
            mode: Mode::Safe,
            outcome: Outcome::Failed,
            reason: Some(Reason::InvalidCandidate),
            original_bytes: 10,
            candidate_bytes: Some(8),
            emitted_bytes: 10,
            original_tokens: None,
            candidate_tokens: None,
            emitted_tokens: None,
            compression_duration: Duration::from_nanos(5),
            validation_outcome: ValidationOutcome::Failed,
        },
        ("failed", "safe", "failed"),
    ));
    for (report, (outcome, mode, validation)) in reports {
        let map = report_map(&report);
        let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
        let mut want = FIELDS.to_vec();
        keys.sort_unstable();
        want.sort_unstable();
        assert_eq!(keys, want, "{outcome}");
        assert_eq!(map["outcome"], outcome);
        assert_eq!(map["mode"], mode, "{outcome}");
        assert_eq!(map["validation_outcome"], validation, "{outcome}");
        assert_eq!(map["schema_version"], 1);
        assert_eq!(map["transform_id"], "json-minify");
        assert_eq!(map["transform_version"], 1);
        assert_eq!(map["policy_version"], 7);
        assert!(map["compression_duration"].is_u64());
    }
}

#[test]
fn changed_report_values() {
    let result = compress(PRETTY, &ctx(Mode::Safe));
    let r = &result.report;
    assert_eq!(result.output, r#"{"a":[1,2,3],"b":"x y"}"#);
    assert!(matches!(result.output, Cow::Owned(_)));
    assert_eq!(r.outcome, Outcome::Changed);
    assert_eq!(r.reason, None);
    assert_eq!(r.validation_outcome, ValidationOutcome::Passed);
    assert_eq!(r.original_bytes, PRETTY.len());
    assert_eq!(r.candidate_bytes, Some(result.output.len()));
    assert_eq!(r.emitted_bytes, result.output.len());
    let map = report_json(&result);
    assert_eq!(map["mode"], "safe");
    assert_eq!(map["skip_or_failure_reason"], serde_json::Value::Null);
    assert_eq!(map["validation_outcome"], "passed");
    assert_eq!(map["format_version"], serde_json::Value::Null);
}

#[test]
fn off_mode_borrows_and_builds_no_candidate() {
    let result = compress(PRETTY, &ctx(Mode::Off));
    assert!(is_borrowed_from(&result, PRETTY));
    let r = &result.report;
    assert_eq!(r.outcome, Outcome::Skipped);
    assert_eq!(r.reason, Some(Reason::ModeOff));
    assert_eq!(r.candidate_bytes, None);
    assert_eq!(r.emitted_bytes, PRETTY.len());
    assert_eq!(r.validation_outcome, ValidationOutcome::NotRun);
    assert_eq!(report_json(&result)["skip_or_failure_reason"], "mode_off");
}

#[test]
fn skips_borrow_input_with_matching_reason() {
    let deep = format!("{}{}", "[ ".repeat(65), "]".repeat(65));
    let deep_ok = format!("{}{}", "[ ".repeat(64), "]".repeat(64));
    let base = ctx(Mode::Safe);
    let disabled = CompressionContext {
        json_minify: false,
        ..base
    };
    let small_max = CompressionContext {
        limits: Limits {
            max_candidate_bytes: 4,
            ..base.limits
        },
        ..base
    };
    let big_min = CompressionContext {
        limits: Limits {
            min_candidate_bytes: 1000,
            ..base.limits
        },
        ..base
    };
    let cases: [(&CompressionContext, &str, Reason); 11] = [
        (&disabled, PRETTY, Reason::TransformDisabled),
        (&big_min, PRETTY, Reason::BelowMinBytes),
        (&small_max, PRETTY, Reason::AboveMaxBytes),
        (&base, "42", Reason::UnsupportedFormat),
        (&base, "{ not json }", Reason::UnsupportedFormat),
        (&base, "{ \"a\": [1, 2", Reason::UnsupportedFormat),
        (&base, "[1] [2]", Reason::UnsupportedFormat),
        (&base, &deep, Reason::DepthLimit),
        (&base, "{ \"a\": 1, \"a\": 2 }", Reason::DuplicateKey),
        (&base, "{ \"\\u0061\": 1 }", Reason::EscapedKey),
        (&base, "[1,2]", Reason::InsufficientGain),
    ];
    for (ctx, input, reason) in cases {
        let result = compress(input, ctx);
        assert!(is_borrowed_from(&result, input), "{reason:?}");
        assert_eq!(result.report.outcome, Outcome::Skipped, "{reason:?}");
        assert_eq!(result.report.reason, Some(reason));
        assert_eq!(result.report.emitted_bytes, input.len());
    }
    assert_eq!(compress(&deep_ok, &base).report.outcome, Outcome::Changed);
}

#[test]
fn reason_names_are_snake_case() {
    for (reason, name) in [
        (Reason::ModeOff, "mode_off"),
        (Reason::TransformDisabled, "transform_disabled"),
        (Reason::BelowMinBytes, "below_min_bytes"),
        (Reason::AboveMaxBytes, "above_max_bytes"),
        (Reason::UnsupportedFormat, "unsupported_format"),
        (Reason::DepthLimit, "depth_limit"),
        (Reason::DuplicateKey, "duplicate_key"),
        (Reason::EscapedKey, "escaped_key"),
        (Reason::InsufficientGain, "insufficient_gain"),
        (Reason::InvalidCandidate, "invalid_candidate"),
        (Reason::HookConflict, "hook_conflict"),
    ] {
        assert_eq!(reason.as_str(), name);
    }
}

#[test]
fn observe_emits_original_and_records_hypothetical_savings() {
    let counter = ByteCounter;
    let ctx = CompressionContext {
        token_counter: Some(&counter),
        ..ctx(Mode::Observe)
    };
    let result = compress(PRETTY, &ctx);
    assert!(is_borrowed_from(&result, PRETTY));
    let r = &result.report;
    assert_eq!(r.outcome, Outcome::WouldChange);
    assert_eq!(r.reason, None);
    assert_eq!(r.validation_outcome, ValidationOutcome::Passed);
    assert_eq!(r.emitted_bytes, r.original_bytes);
    let candidate = r.candidate_bytes.unwrap();
    assert!(candidate < r.original_bytes);
    let tokens = |t: Option<TokenCount>| t.unwrap().count;
    assert_eq!(tokens(r.original_tokens), PRETTY.len() as u64);
    assert_eq!(tokens(r.candidate_tokens), candidate as u64);
    assert_eq!(tokens(r.emitted_tokens), PRETTY.len() as u64);
    let map = report_json(&result);
    assert_eq!(map["mode"], "observe");
    assert_eq!(
        map["candidate_tokens"],
        serde_json::json!({
            "count": candidate,
            "method": "estimate",
            "tokenizer": "test-bytes",
        })
    );
    assert!(compress(PRETTY, &ctx).into_changed().is_none());
}

#[test]
fn safe_mode_emitted_tokens_count_the_candidate() {
    let counter = ByteCounter;
    let ctx = CompressionContext {
        token_counter: Some(&counter),
        ..ctx(Mode::Safe)
    };
    let r = compress(PRETTY, &ctx).report;
    assert_eq!(r.emitted_tokens, r.candidate_tokens);
    assert_ne!(r.emitted_tokens, r.original_tokens);
}

#[test]
fn unknown_token_counts_are_null_not_zero() {
    for mode in [Mode::Off, Mode::Observe, Mode::Safe] {
        let result = compress(PRETTY, &ctx(mode));
        let r = &result.report;
        assert_eq!(
            (r.original_tokens, r.candidate_tokens, r.emitted_tokens),
            (None, None, None)
        );
        let map = report_json(&result);
        for key in ["original_tokens", "candidate_tokens", "emitted_tokens"] {
            assert_eq!(map[key], serde_json::Value::Null, "{key}");
        }
    }
}

#[test]
fn depth_limit_is_capped_for_unbounded_caller_limits() {
    let deep = format!("{}{}", "[".repeat(2000), "]".repeat(2000));
    let ctx = CompressionContext {
        limits: Limits {
            max_json_depth: usize::MAX,
            ..Limits::default()
        },
        ..ctx(Mode::Safe)
    };
    let result = compress(&deep, &ctx);
    assert!(is_borrowed_from(&result, &deep));
    assert_eq!(result.report.reason, Some(Reason::DepthLimit));
}

/// Names a tokenizer with a quote, a backslash and a newline.
struct HostileCounter;

impl TokenCounter for HostileCounter {
    fn count(&self, text: &str) -> TokenCount {
        TokenCount {
            count: text.len() as u64,
            method: TokenMethod::Exact,
            tokenizer: "evil\"name\\v1\n",
        }
    }
}

#[test]
fn report_json_escapes_tokenizer_names() {
    let counter = HostileCounter;
    let ctx = CompressionContext {
        token_counter: Some(&counter),
        ..ctx(Mode::Safe)
    };
    let map = report_json(&compress(PRETTY, &ctx));
    for key in ["original_tokens", "candidate_tokens", "emitted_tokens"] {
        assert_eq!(map[key]["tokenizer"], "evil\"name\\v1\n", "{key}");
        assert_eq!(map[key]["method"], "exact", "{key}");
    }
}

#[test]
fn key_policy_reason_precedes_later_syntax_error() {
    let input = "{ \"a\": 1, \"a\": 2";
    let result = compress(input, &ctx(Mode::Safe));
    assert_eq!(result.report.reason, Some(Reason::DuplicateKey));
    assert!(is_borrowed_from(&result, input));
}
