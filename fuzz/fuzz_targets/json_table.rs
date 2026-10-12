#![no_main]

use deslag_core::{
    FIRST_SLICE_JSON_MINIFY_CONTEXT, Outcome, TableOptions, compress,
    table::{decode, encode},
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(text) = encode(input, &TableOptions::default()) else {
        return;
    };
    let minified = compress(input, &FIRST_SLICE_JSON_MINIFY_CONTEXT);
    assert_ne!(minified.report.outcome, Outcome::Failed);
    // Round trip: decoding yields the minified input, or the input itself when already minimal.
    assert_eq!(decode(&text).as_deref(), Some(minified.output.as_ref()));
});
