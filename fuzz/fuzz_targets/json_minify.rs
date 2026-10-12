#![no_main]

#[path = "common/mod.rs"]
mod common;

use deslag_core::{FIRST_SLICE_JSON_MINIFY_CONTEXT, Outcome, compress};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    let result = compress(input, &FIRST_SLICE_JSON_MINIFY_CONTEXT);
    if result.report.outcome == Outcome::Changed {
        assert!(result.output.len() < input.len());
        assert_eq!(result.output, common::reference_minify(input));
        let again = compress(&result.output, &FIRST_SLICE_JSON_MINIFY_CONTEXT);
        assert_ne!(
            again.report.outcome,
            Outcome::Changed,
            "minify is idempotent"
        );
    }
});
