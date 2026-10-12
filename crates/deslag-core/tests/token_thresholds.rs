use std::borrow::Cow;

use deslag_core::{
    CompressionContext, EXACT_LINE_RUNS_ID, FIRST_SLICE_JSON_MINIFY_CONTEXT, JSON_MINIFY_ID,
    JSON_TABLE_ID, LINE_TEMPLATES_ID, Limits, Mode, Outcome, Reason, TokenCount, TokenCounter,
    TokenMethod, compress,
};

/// Counts bytes as tokens.
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

/// Counts fewer tokens for longer text, so every shorter candidate costs more tokens.
struct Inverse;

impl TokenCounter for Inverse {
    fn count(&self, text: &str) -> TokenCount {
        TokenCount {
            count: 1_000_000 - text.len() as u64,
            method: TokenMethod::Estimate,
            tokenizer: "inverse",
        }
    }
}

fn table_input() -> String {
    let items: Vec<String> = (0..10)
        .map(|i| format!("  {{ \"id\": {i}, \"name\": \"user-{i}\", \"active\": true }}"))
        .collect();
    format!("[\n{}\n]", items.join(",\n"))
}

/// One context and input per transform. Every case changes the input with zero thresholds.
fn cases() -> Vec<(&'static str, CompressionContext<'static>, String)> {
    let base = CompressionContext {
        json_minify: false,
        ..FIRST_SLICE_JSON_MINIFY_CONTEXT
    };
    vec![
        (
            JSON_MINIFY_ID,
            FIRST_SLICE_JSON_MINIFY_CONTEXT,
            "{\n  \"a\": [1, 2, 3],\n  \"b\": \"x y\"\n}\n".to_string(),
        ),
        (
            JSON_TABLE_ID,
            CompressionContext {
                json_table: true,
                ..base
            },
            table_input(),
        ),
        (
            EXACT_LINE_RUNS_ID,
            CompressionContext {
                exact_line_runs: true,
                ..base
            },
            "same line\n".repeat(20),
        ),
        (
            LINE_TEMPLATES_ID,
            CompressionContext {
                line_templates: true,
                ..base
            },
            (0..20)
                .map(|i| format!("connection from 10.0.0.{i} refused\n"))
                .collect(),
        ),
    ]
}

fn with<'c>(
    ctx: &CompressionContext<'c>,
    mode: Mode,
    counter: Option<&'c dyn TokenCounter>,
    tokens: u64,
    permille: u64,
) -> CompressionContext<'c> {
    CompressionContext {
        mode,
        token_counter: counter,
        limits: Limits {
            min_estimated_token_savings: tokens,
            min_estimated_savings_permille: permille,
            ..ctx.limits
        },
        ..*ctx
    }
}

fn outcome(input: &str, ctx: &CompressionContext) -> (Outcome, Option<Reason>) {
    let result = compress(input, ctx);
    if result.report.outcome != Outcome::Changed {
        assert!(matches!(&result.output, Cow::Borrowed(s) if std::ptr::eq(*s, input)));
    }
    (result.report.outcome, result.report.reason)
}

const CHANGED: (Outcome, Option<Reason>) = (Outcome::Changed, None);
const WOULD: (Outcome, Option<Reason>) = (Outcome::WouldChange, None);
const LOW: (Outcome, Option<Reason>) = (Outcome::Skipped, Some(Reason::InsufficientGain));

#[test]
fn every_transform_applies_the_token_thresholds() {
    for (id, ctx, input) in cases() {
        let zero = with(&ctx, Mode::Safe, Some(&Bytes), 0, 0);
        let r = compress(&input, &zero).report;
        assert_eq!((r.transform_id, r.outcome), (id, Outcome::Changed), "{id}");
        let original = r.original_tokens.unwrap().count;
        let saved = original - r.candidate_tokens.unwrap().count;
        let permille = saved * 1000 / original;
        let run = |mode, tokens, permille| {
            outcome(&input, &with(&ctx, mode, Some(&Bytes), tokens, permille))
        };
        assert_eq!(run(Mode::Safe, saved, 0), CHANGED, "{id}");
        assert_eq!(run(Mode::Safe, saved + 1, 0), LOW, "{id}");
        assert_eq!(run(Mode::Safe, 0, permille), CHANGED, "{id}");
        assert_eq!(run(Mode::Safe, 0, permille + 1), LOW, "{id}");
        assert_eq!(run(Mode::Observe, saved, permille), WOULD, "{id}");
        assert_eq!(run(Mode::Observe, saved + 1, 0), LOW, "{id}");
        assert_eq!(run(Mode::Observe, 0, permille + 1), LOW, "{id}");
    }
}

#[test]
fn no_counter_skips_the_token_check() {
    for (id, ctx, input) in cases() {
        let strict = with(&ctx, Mode::Safe, None, u64::MAX, u64::MAX);
        assert_eq!(outcome(&input, &strict), CHANGED, "{id}");
    }
}

#[test]
fn token_regression_is_insufficient_gain() {
    for (id, ctx, input) in cases() {
        let zero = with(&ctx, Mode::Safe, Some(&Inverse), 0, 0);
        assert_eq!(outcome(&input, &zero), LOW, "{id}");
    }
}
