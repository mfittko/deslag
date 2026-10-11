use deslag_core::{
    CompressionContext, DecodeError, FIRST_SLICE_JSON_MINIFY_CONTEXT, Mode, Outcome, Reason,
    ValidationOutcome, compress, decode_lines, encode_lines, encode_templates,
};

const HEADER: &str = "#!deslag lines-v2\n";

fn on(mode: Mode) -> CompressionContext<'static> {
    CompressionContext {
        mode,
        line_templates: true,
        ..FIRST_SLICE_JSON_MINIFY_CONTEXT
    }
}

fn roundtrip(input: &str) -> String {
    let encoded = encode_templates(input).expect("encodable");
    assert_eq!(
        decode_lines(&encoded, input.len()).as_deref(),
        Ok(input),
        "{encoded:?}"
    );
    encoded
}

fn log(n: usize, message: &str, ending: &str) -> String {
    (0..n)
        .map(|i| {
            format!(
                "2026-10-12T10:00:{i:02}Z INFO id={} {message} {i}ms{ending}",
                4700 + i
            )
        })
        .collect()
}

#[test]
fn template_is_encoded_and_decoded() {
    let input = "connection from 10.0.0.1 refused\n\
                 connection from 10.0.0.2 refused\n\
                 connection from 10.0.0.3 refused\n";
    assert_eq!(
        roundtrip(input),
        format!("{HEADER}connection from 10.0.0.1 refused\n#!deslag t3 2\n10.0.0.2\n10.0.0.3\n")
    );
}

#[test]
fn several_variable_positions() {
    let encoded = roundtrip(&log(3, "request served", "\n"));
    assert_eq!(
        encoded,
        format!(
            "{HEADER}2026-10-12T10:00:00Z INFO id=4700 request served 0ms\n\
             #!deslag t3 0,2,5\n\
             2026-10-12T10:00:01Z id=4701 1ms\n\
             2026-10-12T10:00:02Z id=4702 2ms\n"
        )
    );
}

#[test]
fn line_endings_and_edges_round_trip() {
    let lf = log(8, "request served", "\n");
    let crlf = log(8, "request served", "\r\n");
    let cases = [
        String::new(),
        "\n".repeat(30),
        lf.clone(),
        crlf.clone(),
        format!("{lf}{crlf}{lf}"),
        log(8, "x", "\n") + &log(8, "x", "\r\n").replace("\r\n", "\n\r\n"),
        format!("\n\n{lf}\n\n{crlf}\n"),
        format!("{lf}2026-10-12T10:00:99Z INFO id=1 request served 9ms"),
        format!("{crlf}partial"),
        "  a\t1  \n  a\t2  \n  a\t3  \n  a 4  \n".repeat(4),
        "a 1\na 2\n".to_string(),
        "   \n\t\n \t \n".repeat(5),
        "   \r\n\t\r\n \t \r\n".repeat(5),
        format!("{lf} \t \n{lf}"),
        format!("{crlf}\t\r\n  \r\n{crlf}"),
        "a 1\n \n".repeat(6),
    ];
    for input in &cases {
        roundtrip(input);
    }
}

#[test]
fn partial_final_line_stays_literal() {
    let input = format!("{}connection from 10.0.0.9 refused", log(4, "x", "\n"));
    assert!(roundtrip(&input).ends_with("\nconnection from 10.0.0.9 refused"));
}

#[test]
fn constant_token_ends_the_group() {
    let refused = log(10, "connection refused", "\n");
    let reset = log(10, "connection reset", "\n");
    let encoded = roundtrip(&format!("{refused}{reset}"));
    assert_eq!(encoded.matches("#!deslag t10 0,2,5\n").count(), 2);
    let mixed: String = refused
        .lines()
        .zip(reset.lines())
        .map(|(a, b)| format!("{a}\n{b}\n"))
        .collect();
    assert!(!roundtrip(&mixed).contains("#!deslag t"));
    let words = "took 12ms\ntook fast\ntook 13ms\ntook slow\n".repeat(3);
    assert!(!roundtrip(&words).contains("#!deslag t"));
}

#[test]
fn different_gaps_end_the_group() {
    let input = "value  1 of many lines\nvalue 2 of many lines\nvalue\t3 of many lines\n";
    assert_eq!(roundtrip(input), format!("{HEADER}{input}"));
}

#[test]
fn runs_still_use_run_directives() {
    let line = "connection refused by upstream\n";
    assert_eq!(
        roundtrip(&line.repeat(5)),
        format!("{HEADER}{line}#!deslag x5\n")
    );
}

#[test]
fn unprofitable_group_stays_literal() {
    let input = "a 1\na 2\na 3\n";
    assert_eq!(roundtrip(input), format!("{HEADER}{input}"));
}

#[test]
fn marker_lookalikes_are_escaped() {
    let cases = [
        format!("x\n{}", "#!deslag lines-v2\n".repeat(4)),
        format!("x\n{}", "#!deslag lines-v1\n".repeat(3)),
        "#!deslag t2 0\n#!deslag t3 0\n#!deslag t4 0\n#!deslag t5 0\n".repeat(3),
        "#!deslag t2 0\n".to_string(),
        "a 1\n#!deslag t2 0\n2\n".to_string(),
        format!("{}#!deslag x2\n", log(5, "x", "\n")),
        "#!deslag|1 connection refused by upstream\n".repeat(3),
        "#!deslag".to_string(),
    ];
    for input in &cases {
        roundtrip(input);
    }
    let rows: String = (1..=4)
        .map(|i| format!("#!deslag{i} connection refused by upstream\n"))
        .collect();
    assert_eq!(
        roundtrip(&rows),
        format!(
            "{HEADER}#!deslag|#!deslag1 connection refused by upstream\n#!deslag t4 0\n\
             #!deslag|#!deslag2\n#!deslag|#!deslag3\n#!deslag|#!deslag4\n"
        )
    );
}

#[test]
fn already_encoded_input_is_skipped() {
    let v2 = roundtrip(&log(10, "x", "\n"));
    assert_eq!(encode_templates(&v2), None);
    let v1 = encode_lines(&"abcdefghijklmnop\n".repeat(4)).unwrap();
    assert_eq!(encode_templates(&v1), None);
    for input in [v1, v2] {
        let r = compress(&input, &on(Mode::Safe)).report;
        assert_eq!(r.reason, Some(Reason::UnsupportedFormat));
        assert_eq!(r.transform_id, "line-templates");
    }
}

#[test]
fn encoding_is_deterministic() {
    let input = format!(
        "{}{}{}",
        log(20, "served", "\n"),
        log(5, "failed", "\r\n"),
        "done\n".repeat(3)
    );
    let first = roundtrip(&input);
    for _ in 0..10 {
        assert_eq!(encode_templates(&input).unwrap(), first);
        assert_eq!(compress(&input, &on(Mode::Safe)).output, first);
    }
}

#[test]
fn codec_is_opt_in() {
    const { assert!(!FIRST_SLICE_JSON_MINIFY_CONTEXT.line_templates) };
    let input = log(20, "served", "\n");
    let r = compress(&input, &FIRST_SLICE_JSON_MINIFY_CONTEXT);
    assert_eq!(r.report.reason, Some(Reason::UnsupportedFormat));
    assert_eq!(r.report.transform_id, "json-minify");
    assert_eq!(r.output, input);
    let runs_only = CompressionContext {
        exact_line_runs: true,
        ..FIRST_SLICE_JSON_MINIFY_CONTEXT
    };
    let r = compress(&input, &runs_only).report;
    assert_eq!(r.transform_id, "exact-line-runs");
    assert_eq!(r.reason, Some(Reason::InsufficientGain));
}

#[test]
fn compress_reports_changed() {
    let input = format!("starting\n{}done\n", log(30, "request served", "\n"));
    let r = compress(&input, &on(Mode::Safe));
    assert_eq!(r.report.outcome, Outcome::Changed);
    assert_eq!(r.report.transform_id, "line-templates");
    assert_eq!(r.report.transform_version, 1);
    assert_eq!(r.report.format_version, Some(2));
    assert_eq!(r.report.validation_outcome, ValidationOutcome::Passed);
    assert_eq!(r.report.emitted_bytes, r.output.len());
    assert!(r.output.len() < input.len());
    assert_eq!(decode_lines(&r.output, input.len()).unwrap(), input);
    let without_json = CompressionContext {
        json_minify: false,
        ..on(Mode::Safe)
    };
    assert_eq!(compress(&input, &without_json).output, r.output);
}

#[test]
fn lines_v1_output_still_decodes() {
    let input = format!("{}{}", "abcdefghijklmnop\n".repeat(5), log(5, "x", "\n"));
    let v1 = encode_lines(&input).unwrap();
    assert!(v1.starts_with("#!deslag lines-v1\n"));
    assert_eq!(
        decode_lines(&v1, input.len()).as_deref(),
        Ok(input.as_str())
    );
}

#[test]
fn decoder_rejects_malformed_templates() {
    let bad = [
        "#!deslag t2 1\n2\n",
        "a 1\n#!deslag t1 1\n",
        "a 1\n#!deslag t0 1\n",
        "a 1\n#!deslag t02 1\n2\n",
        "a 1\n#!deslag t+2 1\n2\n",
        "a 1\n#!deslag t 1\n2\n",
        "a 1\n#!deslag t2\n2\n",
        "a 1\n#!deslag t2 \n2\n",
        "a 1\n#!deslag t2  1\n2\n",
        "a 1\n#!deslag t2 1\r\n2\n",
        "a 1\n#!deslag t2 1",
        "a 1 b 2\n#!deslag t2 1,1\n2 3\n",
        "a 1 b 2\n#!deslag t2 3,1\n2 3\n",
        "a 1 b 2\n#!deslag t2 01\n2\n",
        "a 1 b 2\n#!deslag t2 1,\n2\n",
        "a 1 b 2\n#!deslag t2 ,1\n2\n",
        "a 1 b 2\n#!deslag t2 4\n2\n",
        "a 1 b 2\n#!deslag t2 99999999999999999999\n2\n",
        "a 1 b 2\n#!deslag t2 18446744073709551615\n2\n",
        "\n#!deslag t2 0\n2\n",
        "a 1\n#!deslag t3 1\n2\n",
        "a 1\n#!deslag t2 1\n2",
        "a 1 b 2\n#!deslag t2 1,3\n2\n",
        "a 1 b 2\n#!deslag t2 1,3\n2 3 4\n",
        "a 1 b 2\n#!deslag t2 1,3\n2  3\n",
        "a 1 b 2\n#!deslag t2 1,3\n 3\n",
        "a 1\n#!deslag t2 1\n\n",
        "a 1\n#!deslag t2 1\n2\t3\n",
        "a 1\n#!deslag t2 1\n#!deslag2\n",
        "a 1\n#!deslag t2 1\n#!deslag|2\n",
        "a 1\n#!deslag t2 1\n2\n#!deslag x2\n",
        "a 1\n#!deslag t2 1\n2\n#!deslag t2 1\n3\n",
        "a 1\n#!deslag x2\n#!deslag t2 1\n3\n",
    ];
    for body in bad {
        let input = format!("{HEADER}{body}");
        assert_eq!(
            decode_lines(&input, 1 << 20),
            Err(DecodeError::InvalidDirective),
            "{input:?}"
        );
    }
    let v1 = "#!deslag lines-v1\na 1\n#!deslag t2 1\n2\n";
    assert_eq!(
        decode_lines(v1, 1 << 20),
        Err(DecodeError::InvalidDirective)
    );
}

#[test]
fn decoder_accepts_valid_templates() {
    let ok = [
        ("a 1\n#!deslag t2 1\n2\n", "a 1\na 2\n"),
        ("a 1\n#!deslag t2 0\nb\n", "a 1\nb 1\n"),
        (
            "#!deslag|#!deslag 1\n#!deslag t2 1\n2\n",
            "#!deslag 1\n#!deslag 2\n",
        ),
        (
            "a 1\n#!deslag t2 0\n#!deslag|#!deslag\n",
            "a 1\n#!deslag 1\n",
        ),
        ("\ta  1 \n#!deslag t2 0,1\nb 2\r\n", "\ta  1 \n\tb  2\r \n"),
        (
            "a 1\n#!deslag t2 1\n2\na 3\n#!deslag x2\n",
            "a 1\na 2\na 3\na 3\n",
        ),
    ];
    for (body, out) in ok {
        let input = format!("{HEADER}{body}");
        assert_eq!(
            decode_lines(&input, 1 << 20).as_deref(),
            Ok(out),
            "{input:?}"
        );
    }
}

#[test]
fn decoder_respects_output_bound() {
    let encoded = format!("{HEADER}a 1 b\n#!deslag t3 1\n2\n3\n");
    assert_eq!(
        decode_lines(&encoded, 18).as_deref(),
        Ok("a 1 b\na 2 b\na 3 b\n")
    );
    assert_eq!(decode_lines(&encoded, 17), Err(DecodeError::OutputTooLarge));
    let many = format!("{HEADER}a 1 b\n#!deslag t1000 1\n{}", "2\n".repeat(999));
    assert_eq!(decode_lines(&many, 100), Err(DecodeError::OutputTooLarge));
}

#[test]
fn generated_inputs_round_trip() {
    const UNITS: [&str; 16] = [
        "t=1 GET /a ok\n",
        " \t \n",
        "  \r\n",
        "t=22 GET /b ok\n",
        "t=3 GET /a fail\n",
        "t=4 GET  /c ok\r\n",
        "\n",
        "c\r\n",
        "#!deslag t2 0\n",
        "#!deslag lines-v2\n",
        "#!deslag1 x y\n",
        "#!deslag|9 x y\n",
        "  a\t9  \n",
        "é 1€\n",
        "#!deslag",
        "partial 7",
    ];
    let mut state = 12345u64;
    for _ in 0..3000 {
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
fn long_unprofitable_group_is_linear() {
    let input: String = (0..100_000).map(|i| format!("{i} {i}\n")).collect();
    assert_eq!(roundtrip(&input), format!("{HEADER}{input}"));
}

#[test]
fn outlier_base_does_not_block_the_group_below() {
    let rows: String = (0..1000)
        .map(|i| format!("12:{:02}:{:02} 200 /a\n", i / 60, i % 60))
        .collect();
    let input = format!("12:00:00 500 /x\n{rows}");
    let encoded = roundtrip(&input);
    assert!(encoded.len() < input.len());
    let r = compress(&input, &on(Mode::Safe));
    assert_eq!(r.report.outcome, Outcome::Changed);
}

#[test]
fn outlier_base_after_a_unique_line_does_not_block_the_group_below() {
    let rows: String = (0..1000)
        .map(|i| format!("12:{:02}:{:02} 200 /a\n", i / 60, i % 60))
        .collect();
    let input = format!("starting\n12:00:00 500 /x\n{rows}");
    let encoded = roundtrip(&input);
    assert!(encoded.len() < input.len());
    let r = compress(&input, &on(Mode::Safe));
    assert_eq!(r.report.outcome, Outcome::Changed);
}

#[test]
fn outlier_base_after_a_failed_pair_does_not_block_the_group_below() {
    let rows: String = (0..1000)
        .map(|i| format!("12:{:02}:{:02} 200 /a\n", i / 60, i % 60))
        .collect();
    let input = format!("x 1\nx 2\n12:00:00 500 /x\n{rows}");
    let encoded = roundtrip(&input);
    assert!(encoded.contains("\n#!deslag t"), "{encoded:.200}");
}

#[test]
fn run_above_the_rows_does_not_block_the_group() {
    let run = "12:00:00 200 /a\n".repeat(20);
    let rows: String = (1..1000)
        .map(|i| format!("12:{:02}:{:02} 200 /a\n", i / 60, i % 60))
        .collect();
    let input = format!("12:00:00 500 /x\n{run}{rows}");
    let encoded = roundtrip(&input);
    let after_run = encoded.split_once("#!deslag x20\n").expect("run").1;
    assert!(
        after_run.starts_with("12:00:01 200 /a\n#!deslag t999 "),
        "{after_run:.80}"
    );
}
