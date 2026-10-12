#![no_main]

use std::borrow::Cow;

#[path = "common/mod.rs"]
mod common;

use deslag_core::{
    CompressionContext, EXACT_LINE_RUNS_ID, FIRST_SLICE_JSON_MINIFY_CONTEXT, JSON_MINIFY_ID,
    JSON_TABLE_ID, Limits, Mode, Outcome, ValidationOutcome, compress, decode_lines, table,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    for mode in [Mode::Off, Mode::Observe, Mode::Safe] {
        let ctx = CompressionContext {
            mode,
            json_table: true,
            exact_line_runs: true,
            limits: Limits {
                min_candidate_bytes: 0,
                ..Limits::default()
            },
            ..FIRST_SLICE_JSON_MINIFY_CONTEXT
        };
        let result = compress(input, &ctx);
        assert_ne!(result.report.validation_outcome, ValidationOutcome::Failed);
        match result.report.outcome {
            Outcome::Changed => {
                assert!(result.output.len() < input.len());
                assert!(matches!(result.output, Cow::Owned(_)));
                let out: &str = &result.output;
                let reference = common::reference_minify(input);
                match result.report.transform_id {
                    JSON_TABLE_ID => {
                        assert_eq!(table::decode(out).as_deref(), Some(reference.as_str()))
                    }
                    EXACT_LINE_RUNS_ID => {
                        assert_eq!(decode_lines(out, input.len()).as_deref(), Ok(input))
                    }
                    JSON_MINIFY_ID => assert_eq!(out, reference),
                    other => panic!("unknown transform {other}"),
                }
            }
            _ => assert!(matches!(&result.output, Cow::Borrowed(s) if std::ptr::eq(*s, input))),
        }
        assert_eq!(result.report.emitted_bytes, result.output.len());
    }
});
