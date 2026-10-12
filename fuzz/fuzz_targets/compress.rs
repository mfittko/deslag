#![no_main]

use std::borrow::Cow;

use deslag_core::{
    CompressionContext, FIRST_SLICE_JSON_MINIFY_CONTEXT, Limits, Mode, Outcome, compress,
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
        match result.report.outcome {
            Outcome::Changed => {
                assert!(result.output.len() < input.len());
                assert!(matches!(result.output, Cow::Owned(_)));
            }
            _ => assert!(matches!(&result.output, Cow::Borrowed(s) if std::ptr::eq(*s, input))),
        }
        assert_eq!(result.report.emitted_bytes, result.output.len());
    }
});
