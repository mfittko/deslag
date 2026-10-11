use deslag_core::{
    CompressionContext, DecodeError, FIRST_SLICE_JSON_MINIFY_CONTEXT, Mode, Outcome, Reason,
    ValidationOutcome, compress, decode_lines, encode_lines,
};

const HEADER: &str = "#!deslag lines-v1\n";
const LINE: &str = "connection refused by upstream\n";

fn on(mode: Mode) -> CompressionContext<'static> {
    CompressionContext {
        mode,
        exact_line_runs: true,
        ..FIRST_SLICE_JSON_MINIFY_CONTEXT
    }
}

fn roundtrip(input: &str) -> String {
    let encoded = encode_lines(input).expect("encodable");
    assert_eq!(decode_lines(&encoded, input.len()).as_deref(), Ok(input));
    encoded
}

#[test]
fn run_is_encoded_and_decoded() {
    let input = LINE.repeat(5);
    assert_eq!(roundtrip(&input), format!("{HEADER}{LINE}#!deslag x5\n"));
}

#[test]
fn line_endings_and_edges_round_trip() {
    let cr = "retry after failure\r\n";
    let cases = [
        "",
        "\n",
        "\n\n\n\n\n\n\n\n\n\n\n\n",
        &cr.repeat(4),
        &format!("{cr}{cr}{cr}{LINE}{LINE}{LINE}"),
        &format!("{LINE}{LINE}{LINE}{LINE}tail"),
        &format!("head\n{}", LINE.repeat(6)),
        &format!("{}tail\n", LINE.repeat(6)),
        &format!("{}{}", LINE.repeat(3), cr.repeat(3)),
        "a\na\na\na\na\na\na\na\na\na\na\na\na",
    ];
    for input in cases {
        roundtrip(input);
    }
}

#[test]
fn terminator_distinguishes_units() {
    let input = format!("{}{}", "abcdefghijklmnop\n".repeat(3), "abcdefghijklmnop");
    assert_eq!(
        roundtrip(&input),
        format!("{HEADER}abcdefghijklmnop\n#!deslag x3\nabcdefghijklmnop")
    );
    let crlf_lf = "abcdefghijklmnop\r\nabcdefghijklmnop\n";
    assert!(!roundtrip(crlf_lf).contains("#!deslag x"));
}

#[test]
fn marker_lookalikes_are_escaped() {
    let cases = [
        "#!deslag x3\n".repeat(4),
        format!("x\n{}", "#!deslag lines-v1\n".repeat(3)),
        "#!deslag|x\n#!deslag x2\n#!deslag\n".to_string(),
        "#!deslag".to_string(),
        format!("{LINE}#!deslag x9\n#!deslag x9\n{LINE}"),
        "#!deslagfoo\n#!deslag x01\n#!deslag y\n".to_string(),
    ];
    for input in &cases {
        roundtrip(input);
    }
    assert_eq!(
        roundtrip(&"#!deslag x3\n".repeat(4)),
        format!("{HEADER}#!deslag|#!deslag x3\n#!deslag x4\n")
    );
}

#[test]
fn short_runs_stay_literal() {
    // The directive `#!deslag x2\n` is 12 bytes. One 2-byte copy does not pay for it.
    let input = "ab\nab\nab\nab\n";
    assert_eq!(roundtrip(input), format!("{HEADER}{input}"));
    let r = compress(input, &on(Mode::Safe));
    assert_eq!(r.report.reason, Some(Reason::InsufficientGain));
}

#[test]
fn no_runs_is_insufficient_gain() {
    let r = compress("one\ntwo\nthree\n", &on(Mode::Safe));
    assert_eq!(r.report.outcome, Outcome::Skipped);
    assert_eq!(r.report.reason, Some(Reason::InsufficientGain));
    assert_eq!(r.output, "one\ntwo\nthree\n");
}

#[test]
fn already_encoded_input_is_skipped() {
    let encoded = roundtrip(&LINE.repeat(5));
    assert_eq!(encode_lines(&encoded), None);
    let r = compress(&encoded, &on(Mode::Safe));
    assert_eq!(r.report.outcome, Outcome::Skipped);
    assert_eq!(r.report.reason, Some(Reason::UnsupportedFormat));
}

#[test]
fn compress_reports_changed() {
    let input = format!("[INFO] start\n{}[INFO] done\n", LINE.repeat(20));
    let r = compress(&input, &on(Mode::Safe));
    assert_eq!(r.report.outcome, Outcome::Changed);
    assert_eq!(r.report.transform_id, "exact-line-runs");
    assert_eq!(r.report.transform_version, 1);
    assert_eq!(r.report.format_version, Some(1));
    assert_eq!(r.report.validation_outcome, ValidationOutcome::Passed);
    assert_eq!(r.report.emitted_bytes, r.output.len());
    assert!(r.output.len() < input.len());
    assert_eq!(decode_lines(&r.output, input.len()).unwrap(), input);
}

#[test]
fn observe_mode_keeps_original() {
    let input = LINE.repeat(20);
    let r = compress(&input, &on(Mode::Observe));
    assert_eq!(r.report.outcome, Outcome::WouldChange);
    assert_eq!(r.output, input);
    assert!(r.report.candidate_bytes.unwrap() < input.len());
    assert_eq!(r.report.format_version, Some(1));
}

#[test]
fn codec_is_opt_in() {
    let input = LINE.repeat(20);
    let r = compress(&input, &FIRST_SLICE_JSON_MINIFY_CONTEXT);
    assert_eq!(r.report.outcome, Outcome::Skipped);
    assert_eq!(r.report.reason, Some(Reason::UnsupportedFormat));
    assert_eq!(r.report.transform_id, "json-minify");
    assert_eq!(r.output, input);
}

#[test]
fn json_stays_on_the_json_path() {
    let r = compress("{ \"a\" : 1 }\n", &on(Mode::Safe));
    assert_eq!(r.report.transform_id, "json-minify");
    assert_eq!(r.output, "{\"a\":1}");
    // JSON the scanner rejects for a key policy is not plain text.
    let dup = "{\"a\":1,\n\"a\":1,\n\"a\":1,\n\"a\":1}";
    let r = compress(dup, &on(Mode::Safe));
    assert_eq!(r.report.reason, Some(Reason::DuplicateKey));
}

#[test]
fn line_codec_works_without_json_minify() {
    let ctx = CompressionContext {
        json_minify: false,
        ..on(Mode::Safe)
    };
    assert_eq!(
        compress(&LINE.repeat(20), &ctx).report.outcome,
        Outcome::Changed
    );
    let neither = CompressionContext {
        exact_line_runs: false,
        ..ctx
    };
    assert_eq!(
        compress("x", &neither).report.reason,
        Some(Reason::TransformDisabled)
    );
}

#[test]
fn decoder_rejects_malformed_input() {
    let bad = [
        ("", DecodeError::MissingHeader),
        ("a\n", DecodeError::MissingHeader),
        ("#!deslag lines-v1", DecodeError::MissingHeader),
        ("#!deslag x3\n", DecodeError::MissingHeader),
        (
            "#!deslag lines-v1\n#!deslag x3\n",
            DecodeError::InvalidDirective,
        ),
        (
            "#!deslag lines-v1\na\n#!deslag x1\n",
            DecodeError::InvalidDirective,
        ),
        (
            "#!deslag lines-v1\na\n#!deslag x0\n",
            DecodeError::InvalidDirective,
        ),
        (
            "#!deslag lines-v1\na\n#!deslag x03\n",
            DecodeError::InvalidDirective,
        ),
        (
            "#!deslag lines-v1\na\n#!deslag x+3\n",
            DecodeError::InvalidDirective,
        ),
        (
            "#!deslag lines-v1\na\n#!deslag x3",
            DecodeError::InvalidDirective,
        ),
        (
            "#!deslag lines-v1\na\n#!deslag x3\r\n",
            DecodeError::InvalidDirective,
        ),
        (
            "#!deslag lines-v1\na\n#!deslag x3\r",
            DecodeError::InvalidDirective,
        ),
        (
            "#!deslag lines-v1\na\n#!deslag x 3\n",
            DecodeError::InvalidDirective,
        ),
        (
            "#!deslag lines-v1\na\n#!deslag x3\n#!deslag x3\n",
            DecodeError::InvalidDirective,
        ),
        (
            "#!deslag lines-v1\na\n#!deslag y\n",
            DecodeError::InvalidDirective,
        ),
        (
            "#!deslag lines-v1\n#!deslag lines-v1\n",
            DecodeError::InvalidDirective,
        ),
        (
            "#!deslag lines-v1\n#!deslag|plain\n",
            DecodeError::InvalidDirective,
        ),
        (
            "#!deslag lines-v1\na\n#!deslag x99999999999999999999999999\n",
            DecodeError::InvalidDirective,
        ),
    ];
    for (input, err) in bad {
        assert_eq!(decode_lines(input, 1 << 20), Err(err), "{input:?}");
    }
}

#[test]
fn decoder_respects_output_bound() {
    let encoded = format!("{HEADER}abcd\n#!deslag x1000000000000\n");
    assert_eq!(
        decode_lines(&encoded, 1 << 20),
        Err(DecodeError::OutputTooLarge)
    );
    let max = format!("{HEADER}ab\n#!deslag x{}\n", usize::MAX);
    assert_eq!(
        decode_lines(&max, usize::MAX),
        Err(DecodeError::OutputTooLarge)
    );
    let ok = format!("{HEADER}ab\n#!deslag x4\n");
    assert_eq!(decode_lines(&ok, 12).as_deref(), Ok("ab\nab\nab\nab\n"));
    assert_eq!(decode_lines(&ok, 11), Err(DecodeError::OutputTooLarge));
}

#[test]
fn generated_inputs_round_trip() {
    const UNITS: [&str; 11] = [
        "a\n",
        "bbbbbbbbbbbbbbbb\n",
        "\n",
        "c\r\n",
        "#!deslag x2\n",
        "#!",
        "#!deslag lines-v1\n",
        "#!deslag|q\n",
        "é€\n",
        "x\ry",
        "#!deslag",
    ];
    let mut state = 12345u64;
    for _ in 0..2000 {
        let mut input = String::new();
        for _ in 0..(state % 24) {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let unit = UNITS[(state >> 33) as usize % UNITS.len()];
            for _ in 0..=((state >> 20) % 5) {
                input.push_str(unit);
            }
        }
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        if !input.starts_with(HEADER) {
            roundtrip(&input);
        }
    }
}

#[test]
fn escaped_copies_count_their_escape() {
    let encoded = roundtrip("#!deslag\n#!deslag\n");
    assert_eq!(encoded, format!("{HEADER}#!deslag|#!deslag\n#!deslag x2\n"));
}

#[test]
fn header_start_input_reports_unsupported_format() {
    let input = format!("{HEADER}{}", LINE.repeat(5));
    let r = compress(&input, &on(Mode::Safe));
    assert_eq!(r.report.transform_id, "exact-line-runs");
    assert_eq!(r.report.format_version, Some(1));
    assert_eq!(r.report.reason, Some(Reason::UnsupportedFormat));
    assert_eq!(r.report.validation_outcome, ValidationOutcome::NotRun);
}

#[test]
fn plain_text_without_runs_reports_insufficient_gain() {
    let r = compress("one\ntwo\nthree\n", &on(Mode::Safe));
    assert_eq!(r.report.reason, Some(Reason::InsufficientGain));
    assert_eq!(r.report.validation_outcome, ValidationOutcome::NotRun);
    assert!(r.report.candidate_bytes.is_some());
}

#[test]
fn malformed_json_falls_through_to_the_codec() {
    for prefix in ["[", "{"] {
        let input = format!("{prefix}\n{}", LINE.repeat(20));
        let r = compress(&input, &on(Mode::Safe));
        assert_eq!(r.report.outcome, Outcome::Changed, "{input:?}");
        assert_eq!(r.report.transform_id, "exact-line-runs");
    }
}
