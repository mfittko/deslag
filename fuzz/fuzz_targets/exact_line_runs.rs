#![no_main]

use deslag_core::{decode_lines, encode_lines};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    if let Some(encoded) = encode_lines(input) {
        assert_eq!(decode_lines(&encoded, input.len()).as_deref(), Ok(input));
    }
});
