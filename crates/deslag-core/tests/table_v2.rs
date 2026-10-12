use deslag_core::{
    CompressionContext, FIRST_SLICE_JSON_MINIFY_CONTEXT, JSON_TABLE_FORMAT_VERSION, JSON_TABLE_ID,
    JSON_TABLE_V2_FORMAT_VERSION, JSON_TABLE_VERSION, Mode, Outcome, Reason, TableOptions,
    ValidationOutcome, compress, minify_json,
    table::{self, v2},
};

fn lenient() -> TableOptions<'static> {
    TableOptions {
        min_savings_permille: 0,
        ..TableOptions::default()
    }
}

fn minified(input: &str) -> String {
    minify_json(input).unwrap_or_else(|| input.to_string())
}

/// Encodes with `lenient` options and asserts the round trip. Returns the text.
fn round_trip(input: &str) -> String {
    let encoded = v2::encode(input, &lenient()).unwrap_or_else(|r| panic!("{r:?}: {input}"));
    assert_eq!(v2::decode(&encoded), Some(minified(input)), "{encoded}");
    encoded
}

/// `n` uniform rows as a JSON array.
fn rows(n: usize) -> String {
    let items: Vec<String> = (0..n)
        .map(|i| format!(r#"{{"id":{i},"name":"user-{i}"}}"#))
        .collect();
    format!("[{}]", items.join(","))
}

#[test]
fn identity_is_stable() {
    assert_eq!(JSON_TABLE_VERSION, 5);
    assert_eq!(JSON_TABLE_FORMAT_VERSION, 1);
    assert_eq!(JSON_TABLE_V2_FORMAT_VERSION, 2);
    assert_eq!(v2::MARKER, "table-v2");
}

#[test]
fn wrapper_fields_before_and_after_the_array() {
    let input = format!(
        "{{ \"page\": 1, \"results\": {}, \"total\": 1000, \"next\": null }}",
        rows(3)
    );
    assert_eq!(
        round_trip(&input),
        "table-v2 {\"page\":1,\"results\":@0,\"total\":1000,\"next\":null}\n\
         @0 [3]{id:int,name:str}\n0,user-0\n1,user-1\n2,user-2"
    );
}

#[test]
fn wrapper_with_only_the_array() {
    let input = format!("{{\"results\":{}}}", rows(3));
    assert!(round_trip(&input).starts_with("table-v2 {\"results\":@0}\n@0 [3]"));
}

#[test]
fn several_wrapper_fields_and_arrays() {
    let input = format!(
        r#"{{"a":true,"users":{},"b":"x","groups":{},"c":[1,2],"d":{{"e":null}}}}"#,
        rows(4),
        rows(5)
    );
    let encoded = round_trip(&input);
    assert!(
        encoded.starts_with(
            "table-v2 {\"a\":true,\"users\":@0,\"b\":\"x\",\"groups\":@1,\"c\":[1,2],\"d\":{\"e\":null}}\n@0 [4]"
        ),
        "{encoded}"
    );
    assert!(encoded.contains("\n@1 [5]"), "{encoded}");
}

#[test]
fn wrapper_lexemes_survive_verbatim() {
    // Non-canonical escapes, number spellings, nested arrays and `@0` inside
    // strings stay in the skeleton byte for byte.
    let input = format!(
        r#"{{"meta":{{"q":"a\/b \u00e9 @0","n":1.0e3,"z":-0,"tags":[1,{{"x":[]}},"@1"]}},"@0":{},"tail":"\"@"}}"#,
        rows(3)
    );
    let encoded = round_trip(&input);
    assert!(encoded.contains(r#""@0":@0,"#), "{encoded}");
}

#[test]
fn table_rows_inside_a_wrapper_keep_nested_columns() {
    let input = r#"{"items":[{"id":1,"m":{"r":"eu"}},{"id":2,"m":{"r":"us"}},{"id":3,"m":{"r":""}}],"n":3}"#;
    let encoded = round_trip(input);
    assert!(encoded.contains("@0 [3]{id:int,m.r:str}"), "{encoded}");
}

#[test]
fn ineligible_arrays_stay_inline() {
    // A one-row array and an array of scalars stay in the skeleton.
    let input = format!(r#"{{"one":[{{"a":1}}],"nums":[1,2,3],"rows":{}}}"#, rows(4));
    let encoded = round_trip(&input);
    assert!(
        encoded.starts_with(r#"table-v2 {"one":[{"a":1}],"nums":[1,2,3],"rows":@0}"#),
        "{encoded}"
    );
}

#[test]
fn bare_array_keeps_table_v1_output() {
    let input = rows(5);
    let v1 = table::encode(&input, &lenient()).unwrap();
    assert_eq!(v2::encode(&input, &lenient()), Ok(v1.clone()));
    assert!(v1.starts_with("table-v1 "));
    // The v2 decoder reads table-v1 text unchanged.
    assert_eq!(v2::decode(&v1), table::decode(&v1));
    assert_eq!(v2::decode(&v1), Some(input));
    // Bare-array rejections keep their table-v1 reason.
    assert_eq!(
        v2::encode(
            &rows(5),
            &TableOptions {
                min_savings_permille: 900,
                ..lenient()
            }
        ),
        Err(Reason::InsufficientGain)
    );
}

#[test]
fn table_v1_decoder_rejects_table_v2_text() {
    let encoded = round_trip(&format!("{{\"r\":{}}}", rows(3)));
    assert_eq!(table::decode(&encoded), None);
}

#[test]
fn ineligible_wrappers() {
    let reject = |input: &str, opts: &TableOptions, reason| {
        assert_eq!(v2::encode(input, opts), Err(reason), "{input}");
    };
    let opts = lenient();
    reject(r#"{"a":1,"b":[1,2]}"#, &opts, Reason::UnsupportedFormat);
    reject(r#"{}"#, &opts, Reason::UnsupportedFormat);
    reject(r#""text""#, &opts, Reason::UnsupportedFormat);
    reject("not json", &opts, Reason::UnsupportedFormat);
    reject(
        &format!(r#"{{"a":1,"a":2,"r":{}}}"#, rows(3)),
        &opts,
        Reason::DuplicateKey,
    );
    reject(
        // The key is `a`, an escaped `a`.
        &format!(r#"{{"{}u0061":1,"r":{}}}"#, '\\', rows(3)),
        &opts,
        Reason::EscapedKey,
    );
    let deep = format!(r#"{{"d":[[[[1]]]],"r":{}}}"#, rows(3));
    let shallow = TableOptions {
        max_depth: 3,
        ..lenient()
    };
    reject(&deep, &shallow, Reason::DepthLimit);
    let few = TableOptions {
        max_rows: 2,
        ..lenient()
    };
    reject(
        &format!("{{\"r\":{}}}", rows(3)),
        &few,
        Reason::UnsupportedFormat,
    );
    // A large wrapper outweighs the saving on a small array.
    let big = format!(r#"{{"blob":"{}","r":{}}}"#, "x".repeat(4000), rows(3));
    reject(&big, &TableOptions::default(), Reason::InsufficientGain);
}

#[test]
fn max_rows_applies_per_array() {
    let opts = TableOptions {
        max_rows: 5,
        ..lenient()
    };
    let input = format!(r#"{{"big":{},"small":{}}}"#, rows(6), rows(5));
    let encoded = v2::encode(&input, &opts).unwrap_or_else(|r| panic!("{r:?}"));
    let skeleton = encoded.lines().next().unwrap();
    assert_eq!(
        skeleton,
        format!(r#"table-v2 {{"big":{},"small":@0}}"#, rows(6))
    );
    assert_eq!(encoded.matches("\n@").count(), 1, "{encoded}");
    assert_eq!(v2::decode(&encoded), Some(minified(&input)));
}

#[test]
fn decoder_rejects_malformed_v2_text() {
    let block = "[2]{a:int}\n1\n2";
    let bad = [
        "table-v2".to_string(),
        "table-v2 ".to_string(),
        "table-v2 {\"r\":[1]}".to_string(),
        // No hole.
        format!("table-v2 {{\"r\":[1]}}\n@0 {block}"),
        // Missing block.
        "table-v2 {\"r\":@0}".to_string(),
        // Extra block.
        format!("table-v2 {{\"r\":@0}}\n@0 {block}\n@1 {block}"),
        // Holes out of order, repeated, or not canonical.
        format!("table-v2 {{\"r\":@1}}\n@1 {block}"),
        format!("table-v2 {{\"r\":@0,\"s\":@0}}\n@0 {block}\n@0 {block}"),
        format!("table-v2 {{\"r\":@00}}\n@00 {block}"),
        format!("table-v2 {{\"r\":@}}\n@ {block}"),
        // Block label mismatch or missing separator.
        format!("table-v2 {{\"r\":@0}}\n@1 {block}"),
        format!("table-v2 {{\"r\":@0}}\n@0{block}"),
        format!("table-v2 {{\"r\":@0}}@0 {block}"),
        // Hole in a key position, or adjacent to a value.
        format!("table-v2 {{@0:1}}\n@0 {block}"),
        format!("table-v2 {{\"r\":1@0}}\n@0 {block}"),
        // Skeleton is not minified JSON.
        format!("table-v2 {{\"r\": @0}}\n@0 {block}"),
        format!("table-v2 {{\"r\":@0,}}\n@0 {block}"),
        format!("table-v2 {{\"r\":@0,\"r\":1}}\n@0 {block}"),
        format!("table-v2 \"s\"@0\n@0 {block}"),
        // Malformed block.
        "table-v2 {\"r\":@0}\n@0 [3]{a:int}\n1\n2".to_string(),
        "table-v2 {\"r\":@0}\n@0 [2]{a:int}\n1\n2\n".to_string(),
        "table-v2 {\"r\":@0}\n@0 [2]{a:int}\n1\nx".to_string(),
        format!("table-v2 {{\"r\":@0}}\n@0 table-v1 {block}"),
        // Hole inside a string is text, so the block is extra.
        format!("table-v2 {{\"r\":\"@0\"}}\n@0 {block}"),
        // Trailing text.
        format!("table-v2 {{\"r\":@0}}\n@0 {block}\nx"),
        format!(" table-v2 {{\"r\":@0}}\n@0 {block}"),
    ];
    for text in &bad {
        assert_eq!(v2::decode(text), None, "{text:?}");
    }
    let good = format!("table-v2 {{\"r\":@0}}\n@0 {block}");
    assert_eq!(
        v2::decode(&good).as_deref(),
        Some(r#"{"r":[{"a":1},{"a":2}]}"#)
    );
}

#[test]
fn decoder_accepts_non_canonical_holes_and_zero_row_blocks() {
    let cases = [
        ("table-v2 @0\n@0 [1]{a:int}\n1", r#"[{"a":1}]"#),
        ("table-v2 [1,@0]\n@0 [1]{a:int}\n1", r#"[1,[{"a":1}]]"#),
        (
            "table-v2 {\"o\":{\"r\":@0}}\n@0 [1]{a:int}\n1",
            r#"{"o":{"r":[{"a":1}]}}"#,
        ),
        ("table-v2 {\"r\":@0}\n@0 [0]{a:int}", r#"{"r":[]}"#),
    ];
    for (text, expected) in cases {
        assert_eq!(v2::decode(text).as_deref(), Some(expected), "{text:?}");
    }
}

#[test]
fn decoder_reads_blocks_with_quoted_newlines() {
    let input = r#"{"r":[{"t":"a\nb"},{"t":"@0\n@1 [1]{x:int}"},{"t":"plain"},{"t":"plain"},{"t":"plain"},{"t":"plain"}],"z":1}"#;
    let encoded = round_trip(input);
    assert!(encoded.contains("\"a\nb\""), "{encoded}");
}

/// Tiny deterministic LCG, so the property test needs no dependency.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.next() as usize % items.len()]
    }
}

#[test]
fn generated_wrappers_round_trip() {
    const SCALARS: &[&str] = &[
        "null",
        "true",
        "0",
        "-0",
        "1.0e3",
        "12345678901234567890123",
        r#""""#,
        r#""@0""#,
        r#""a\/b""#,
        r#""\u00e9🦀""#,
        r#""x\ny""#,
        "[]",
        "{}",
        r#"[1,{"k":"@1"}]"#,
    ];
    // One cell list per column type; `null` fits either.
    const CELLS: [&[&str]; 2] = [
        &["null", "1", "-7", "0", "12345678901234567890123"],
        &[
            "null",
            r#""""#,
            r#""plain""#,
            r#""a,b""#,
            r#""q\"q""#,
            r#""l\nm""#,
            r#""@0""#,
            r#""1e3""#,
        ],
    ];
    let mut rng = Lcg(0xa11ce);
    let (mut encoded_count, mut multi) = (0, 0);
    for _ in 0..300 {
        let nfields = 1 + rng.next() as usize % 6;
        let mut fields = Vec::new();
        for f in 0..nfields {
            let value = if rng.next().is_multiple_of(2) {
                let nrows = 1 + rng.next() as usize % 6;
                let ncols = 1 + rng.next() as usize % 3;
                let types: Vec<usize> = (0..ncols).map(|_| rng.next() as usize % 2).collect();
                let items: Vec<String> = (0..nrows)
                    .map(|_| {
                        let mut cells = Vec::new();
                        for (c, &t) in types.iter().enumerate() {
                            if !rng.next().is_multiple_of(6) {
                                cells.push(format!("\"c{c}\":{}", rng.pick(CELLS[t])));
                            }
                        }
                        format!("{{{}}}", cells.join(","))
                    })
                    .collect();
                format!("[{}]", items.join(","))
            } else {
                rng.pick(SCALARS).to_string()
            };
            fields.push(format!("\"f{f}\":{value}"));
        }
        let input = format!("{{{}}}", fields.join(","));
        match v2::encode(&input, &lenient()) {
            Ok(encoded) => {
                encoded_count += 1;
                multi += usize::from(encoded.contains("\n@1 "));
                assert!(encoded.starts_with("table-v2 "), "{encoded}");
                assert_eq!(
                    v2::decode(&encoded).as_deref(),
                    Some(input.as_str()),
                    "{encoded}"
                );
            }
            Err(r) => assert!(
                matches!(r, Reason::InsufficientGain | Reason::UnsupportedFormat),
                "{r:?}: {input}"
            ),
        }
    }
    assert!(encoded_count > 100, "{encoded_count}");
    assert!(multi > 20, "{multi}");
}

fn table_ctx(mode: Mode) -> CompressionContext<'static> {
    CompressionContext {
        mode,
        json_table: true,
        ..FIRST_SLICE_JSON_MINIFY_CONTEXT
    }
}

#[test]
fn compress_reports_table_v2_identity() {
    let input = format!("{{\n  \"results\": {},\n  \"total\": 1000\n}}", rows(40));
    let result = compress(&input, &table_ctx(Mode::Safe));
    let r = &result.report;
    assert_eq!(r.outcome, Outcome::Changed);
    assert_eq!(r.transform_id, JSON_TABLE_ID);
    assert_eq!(r.transform_version, JSON_TABLE_VERSION);
    assert_eq!(r.format_version, Some(JSON_TABLE_V2_FORMAT_VERSION));
    assert_eq!(r.validation_outcome, ValidationOutcome::Passed);
    assert!(
        result
            .output
            .starts_with("table-v2 {\"results\":@0,\"total\":1000}\n@0 [40]")
    );
    assert_eq!(v2::decode(&result.output), Some(minified(&input)));

    let observe = compress(&input, &table_ctx(Mode::Observe));
    assert_eq!(observe.report.outcome, Outcome::WouldChange);
    assert_eq!(
        observe.report.format_version,
        Some(JSON_TABLE_V2_FORMAT_VERSION)
    );
    assert_eq!(observe.output, input);
}

#[test]
fn compress_keeps_table_v1_identity_for_bare_arrays() {
    let input = rows(40);
    let result = compress(&input, &table_ctx(Mode::Safe));
    assert_eq!(result.report.outcome, Outcome::Changed);
    assert_eq!(
        result.report.format_version,
        Some(JSON_TABLE_FORMAT_VERSION)
    );
    assert!(result.output.starts_with("table-v1 [40]"));
}

#[test]
fn compress_falls_back_to_minify_for_ineligible_wrappers() {
    let input = "{ \"a\": 1, \"b\": [1, 2] }";
    let result = compress(input, &table_ctx(Mode::Safe));
    assert_eq!(result.report.outcome, Outcome::Changed);
    assert_eq!(result.report.transform_id, "json-minify");
    assert_eq!(result.report.format_version, None);
    let table_only = CompressionContext {
        json_minify: false,
        ..table_ctx(Mode::Safe)
    };
    let result = compress(input, &table_only);
    assert_eq!(result.report.reason, Some(Reason::UnsupportedFormat));
    assert_eq!(result.report.transform_id, JSON_TABLE_ID);
}
