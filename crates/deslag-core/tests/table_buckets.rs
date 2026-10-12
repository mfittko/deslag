use deslag_core::{
    CompressionContext, FIRST_SLICE_JSON_MINIFY_CONTEXT, JSON_TABLE_FORMAT_VERSION, JSON_TABLE_ID,
    JSON_TABLE_V2_FORMAT_VERSION, Mode, Outcome, Reason, TableOptions, ValidationOutcome, compress,
    minify_json,
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

/// Rows whose `v` field is an int for kind `a` and a string for kind `b`,
/// so one table-v1 table cannot hold them.
fn mixed(n: usize) -> String {
    let items: Vec<String> = (0..n)
        .map(|i| {
            if i % 3 == 1 {
                format!(r#"{{"type":"b","v":"s{i}"}}"#)
            } else {
                format!(r#"{{"type":"a","v":{i}}}"#)
            }
        })
        .collect();
    format!("[{}]", items.join(","))
}

#[test]
fn bare_interleaved_kinds() {
    let input = mixed(6);
    assert_eq!(
        table::encode(&input, &lenient()),
        Err(Reason::UnsupportedFormat)
    );
    assert_eq!(
        round_trip(&input),
        "table-v2 @0\n@0 [6] by type\n0,1,0,0,1,0\n\
         @0.0 [4]{type:str,v:int}\na,0\na,2\na,3\na,5\n\
         @0.1 [2]{type:str,v:str}\nb,s1\nb,s4"
    );
}

#[test]
fn wrapped_interleaved_kinds() {
    let input = format!(r#"{{"page":1,"events":{},"more":false}}"#, mixed(8));
    let encoded = round_trip(&input);
    assert!(
        encoded.starts_with(
            "table-v2 {\"page\":1,\"events\":@0,\"more\":false}\n@0 [8] by type\n0,1,0,0,1,0,0,1\n@0.0 [5]"
        ),
        "{encoded}"
    );
    assert!(encoded.contains("\n@0.1 [3]{type:str,v:str}"), "{encoded}");
}

#[test]
fn kinds_with_one_row_missing_fields_and_nested_columns() {
    // `click` appears once; `view` rows miss `ms` sometimes; each kind has its
    // own nested object shape, which one table-v1 table rejects.
    let input = r#"[
        {"kind":"view","id":1,"page":{"path":"/a","q":null},"ms":3},
        {"kind":"error","id":2,"err":{"code":500,"msg":"x,y"}},
        {"kind":"view","id":3,"page":{"path":"/b","q":"z"}},
        {"kind":"click","id":4,"at":{"x":1.5,"y":-0}},
        {"kind":"error","id":5,"err":{"code":404,"msg":""}},
        {"kind":"view","id":6,"page":{"path":"/c","q":null},"ms":12}
    ]"#;
    let encoded = round_trip(input);
    assert!(
        encoded.starts_with("table-v2 @0\n@0 [6] by kind\n0,1,0,2,1,0\n"),
        "{encoded}"
    );
    assert!(encoded.contains("\n@0.0 [3]{kind:str,id:int,page.path:str,page.q:str,ms:int}\n"));
    assert!(encoded.contains("\n@0.2 [1]{kind:str,id:int,at.x:num,at.y:int}\nclick,4,1.5,-0"));
}

#[test]
fn several_wrapped_arrays_number_their_buckets_by_hole() {
    let input = format!(r#"{{"x":{},"y":{}}}"#, mixed(8), mixed(9));
    let encoded = round_trip(&input);
    for label in ["\n@0 [8] by type", "\n@0.1 ", "\n@1 [9] by type", "\n@1.1 "] {
        assert!(encoded.contains(label), "{label}: {encoded}");
    }
}

#[test]
fn discriminator_is_the_shortest_candidate() {
    // `name` is unique per row, `status` has two values that do not match
    // the shapes, and `type` matches them. `type` gives the shortest block.
    let items: Vec<String> = (0..9)
        .map(|i| {
            let status = if i % 2 == 0 { "ok" } else { "late" };
            if i % 3 == 0 {
                format!(r#"{{"name":"n{i}","status":"{status}","type":"num","v":{i}}}"#)
            } else {
                format!(r#"{{"name":"n{i}","status":"{status}","type":"txt","v":"t{i}"}}"#)
            }
        })
        .collect();
    let encoded = round_trip(&format!("[{}]", items.join(",")));
    assert!(
        encoded.starts_with("table-v2 @0\n@0 [9] by type\n"),
        "{encoded}"
    );
}

#[test]
fn key_unique_in_every_row_is_not_a_discriminator() {
    // `id` is the only candidate and every row has its own value, so the
    // array has no bucket block. Without the skip the encoder builds one.
    let items: Vec<String> = (0..4)
        .map(|i| format!(r#"{{"id":"u{i}","a":{{"b":{{"c":{{"d":{{"e{i}":{i}}}}}}}}}}}"#))
        .collect();
    let input = format!("[{}]", items.join(","));
    assert_eq!(
        v2::encode(&input, &lenient()),
        Err(Reason::UnsupportedFormat)
    );
}

#[test]
fn discriminator_tie_goes_to_the_first_key() {
    // `tag` and `knd` have names of equal length and split the rows the same
    // way, so both bucket blocks have the same length. The first key wins.
    let items: Vec<String> = (0..8)
        .map(|i| {
            if i % 2 == 0 {
                format!(r#"{{"tag":"p","knd":"a","v":{i}}}"#)
            } else {
                format!(r#"{{"tag":"q","knd":"b","v":"s{i}"}}"#)
            }
        })
        .collect();
    let input = format!("[{}]", items.join(","));
    let encoded = round_trip(&input);
    assert!(
        encoded.starts_with("table-v2 @0\n@0 [8] by tag\n"),
        "{encoded}"
    );
    let other = encoded.replacen(" by tag\n", " by knd\n", 1);
    assert_eq!(other.len(), encoded.len());
    assert_eq!(v2::decode(&other), Some(input));
}

#[test]
fn only_the_first_8_candidates_are_tried() {
    // Rows whose leading string keys never split, followed by `type`, which
    // does. With `type` as the 8th candidate it splits; as the 9th it is
    // never tried, so the array has no bucket block.
    let rows = |constant: usize| {
        let items: Vec<String> = (0..6)
            .map(|i| {
                let lead: String = (0..constant).map(|k| format!(r#""k{k}":"c","#)).collect();
                if i % 2 == 0 {
                    format!(r#"{{{lead}"type":"a","v":{i}}}"#)
                } else {
                    format!(r#"{{{lead}"type":"b","v":"s{i}"}}"#)
                }
            })
            .collect();
        format!("[{}]", items.join(","))
    };
    assert!(round_trip(&rows(7)).starts_with("table-v2 @0\n@0 [6] by type\n"));
    assert_eq!(
        v2::encode(&rows(8), &lenient()),
        Err(Reason::UnsupportedFormat)
    );
}

#[test]
fn non_string_and_partial_fields_are_not_candidates() {
    // `id` is a number and `opt` is missing in some rows; only `t` remains.
    let items: Vec<String> = (0..9)
        .map(|i| match i % 3 {
            0 => format!(r#"{{"id":{i},"opt":"o","t":"x","v":{i}}}"#),
            1 => format!(r#"{{"id":{i},"t":"y","v":"s{i}"}}"#),
            _ => format!(r#"{{"id":{i},"t":"x","v":{i}}}"#),
        })
        .collect();
    let input = format!("[{}]", items.join(","));
    assert!(
        round_trip(&input).starts_with("table-v2 @0\n@0 [9] by t\n0,1,0,0,1,0,0,1,0\n"),
        "{input}"
    );
    // No string field in every row: nothing to bucket by.
    let none = r#"[{"t":1,"v":1},{"t":2,"v":"s"}]"#;
    assert_eq!(v2::encode(none, &lenient()), Err(Reason::UnsupportedFormat));
    // A single kind whose rows conflict stays ineligible.
    let one = r#"[{"t":"a","v":1},{"t":"a","v":"s"}]"#;
    assert_eq!(v2::encode(one, &lenient()), Err(Reason::UnsupportedFormat));
}

#[test]
fn homogeneous_arrays_keep_their_output() {
    // A low-cardinality string field does not trigger bucketing when one
    // table holds every row.
    let items: Vec<String> = (0..6)
        .map(|i| format!(r#"{{"id":{i},"status":"{}"}}"#, ["ok", "err"][i % 2]))
        .collect();
    let bare = format!("[{}]", items.join(","));
    let v1 = table::encode(&bare, &lenient()).unwrap();
    assert_eq!(v2::encode(&bare, &lenient()), Ok(v1));
    let wrapped = format!(r#"{{"r":{bare}}}"#);
    assert_eq!(
        round_trip(&wrapped),
        "table-v2 {\"r\":@0}\n@0 [6]{id:int,status:str}\n0,ok\n1,err\n2,ok\n3,err\n4,ok\n5,err"
    );
    // A sparse table-v1 table that beats buckets also stays.
    let sparse = r#"[{"t":"a","x":1},{"t":"b","y":2},{"t":"a","x":3},{"t":"b","y":4}]"#;
    assert_eq!(
        v2::encode(sparse, &lenient()),
        table::encode(sparse, &lenient())
    );
}

#[test]
fn bucket_limits_and_gain() {
    let few = TableOptions {
        max_rows: 8,
        ..lenient()
    };
    assert_eq!(v2::encode(&mixed(9), &few), Err(Reason::UnsupportedFormat));
    assert!(v2::encode(&mixed(8), &few).is_ok());
    let strict = TableOptions {
        min_savings_permille: 900,
        ..lenient()
    };
    assert_eq!(
        v2::encode(&mixed(6), &strict),
        Err(Reason::InsufficientGain)
    );
}

#[test]
fn decoder_rejects_malformed_bucket_blocks() {
    let a = "@0.0 [2]{t:str,v:int}\na,1\na,2";
    let b = "@0.1 [1]{t:str,v:str}\nb,s";
    let good = format!("table-v2 @0\n@0 [3] by t\n0,1,0\n{a}\n{b}");
    assert_eq!(
        v2::decode(&good).as_deref(),
        Some(r#"[{"t":"a","v":1},{"t":"b","v":"s"},{"t":"a","v":2}]"#)
    );
    let bad = [
        // Order line: out of range, not first-seen order, bad numbers, length.
        format!("table-v2 @0\n@0 [3] by t\n0,2,0\n{a}\n{b}"),
        format!("table-v2 @0\n@0 [3] by t\n1,0,1\n{a}\n{b}"),
        format!("table-v2 @0\n@0 [3] by t\n0,01,0\n{a}\n{b}"),
        format!("table-v2 @0\n@0 [3] by t\n0,,1\n{a}\n{b}"),
        format!("table-v2 @0\n@0 [3] by t\n0, 1,0\n{a}\n{b}"),
        format!("table-v2 @0\n@0 [3] by t\n0,1\n{a}\n{b}"),
        format!("table-v2 @0\n@0 [3] by t\n0,1,0,1\n{a}\n{b}"),
        format!("table-v2 @0\n@0 [3] by t\n{a}\n{b}"),
        "table-v2 @0\n@0 [3] by t".to_string(),
        // Bucket row count does not match the order line.
        format!("table-v2 @0\n@0 [3] by t\n0,1,1\n{a}\n{b}"),
        format!("table-v2 @0\n@0 [3] by t\n0,1,0\n@0.0 [1]{{t:str,v:int}}\na,1\n{b}"),
        format!("table-v2 @0\n@0 [3] by t\n0,1,0\n{a}\n@0.1 [2]{{t:str,v:str}}\nb,s"),
        // Bucket with no rows, missing or extra bucket.
        format!("table-v2 @0\n@0 [3] by t\n0,1,0\n{a}\n{b}\n@0.2 [0]{{t:str}}"),
        format!("table-v2 @0\n@0 [3] by t\n0,1,0\n{a}"),
        format!("table-v2 @0\n@0 [2] by t\n0,0\n{a}\n{b}"),
        // Count, header and labels.
        format!("table-v2 @0\n@0 [0] by t\n\n{a}"),
        format!("table-v2 @0\n@0 [03] by t\n0,1,0\n{a}\n{b}"),
        format!("table-v2 @0\n@0 [3] by \n0,1,0\n{a}\n{b}"),
        format!("table-v2 @0\n@0 [3] by a.t\n0,1,0\n{a}\n{b}"),
        format!("table-v2 @0\n@0 [3]by t\n0,1,0\n{a}\n{b}"),
        format!("table-v2 @0\n@0 [3] by x\n0,1,0\n{a}\n{b}"),
        format!("table-v2 @0\n@0 [3] by t\n0,1,0\n{a}\n@0.2 [1]{{t:str,v:str}}\nb,s"),
        format!("table-v2 @0\n@0 [3] by t\n0,1,0\n{a}\n@1.1 [1]{{t:str,v:str}}\nb,s"),
        format!("table-v2 @0\n@0 [3] by t\n0,1,0\n{a}\n@0.1[1]{{t:str,v:str}}\nb,s"),
        format!("table-v2 @0\n@0 [3] by t\n0,1,0\n{a}\n@0.1 {b}"),
        // Malformed bucket table, nested bucket, trailing text.
        format!("table-v2 @0\n@0 [3] by t\n0,1,0\n{a}\n@0.1 [1]{{t:str,v:int}}\nb,s"),
        format!("table-v2 @0\n@0 [3] by t\n0,1,0\n{a}\n@0.1 [1] by t\n0\n@0.1.0 [1]{{t:str}}\nb"),
        format!("{good}\n"),
        format!("{good}\nx"),
        // The interleaved result must be valid minified JSON.
        format!("table-v2 {{@0:1}}\n@0 [3] by t\n0,1,0\n{a}\n{b}"),
    ];
    for text in &bad {
        assert_eq!(v2::decode(text), None, "{text:?}");
    }
}

#[test]
fn decoder_accepts_non_canonical_bucket_blocks() {
    // One bucket, a non-string discriminator column, and mixed values in a
    // bucket all decode; the encoder never emits them.
    let cases = [
        (
            "table-v2 @0\n@0 [1] by t\n0\n@0.0 [1]{t:str}\na",
            r#"[{"t":"a"}]"#,
        ),
        (
            "table-v2 @0\n@0 [2] by t\n0,1\n@0.0 [1]{t:int}\n1\n@0.1 [1]{t:int}\n1",
            r#"[{"t":1},{"t":1}]"#,
        ),
        (
            "table-v2 @0\n@0 [2] by t\n0,0\n@0.0 [2]{t:str}\na\nb",
            r#"[{"t":"a"},{"t":"b"}]"#,
        ),
    ];
    for (text, expected) in cases {
        assert_eq!(v2::decode(text).as_deref(), Some(expected), "{text:?}");
    }
}

#[test]
fn quoted_cells_may_look_like_bucket_labels() {
    let tail = mixed(8);
    let head =
        r#"[{"type":"b","v":"x\n@0.1 [1]{type:str}"},{"type":"a","v":1},{"type":"b","v":"@0.1"},"#;
    let input = format!("{head}{}", &tail[1..]);
    let encoded = round_trip(&input);
    assert!(
        encoded.contains("\n@0.0 [5]{type:str,v:str}\nb,\"x\n@0.1 [1]{type:str}\"\n"),
        "{encoded}"
    );
}

/// Tiny deterministic LCG, so the property test needs no dependency.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.next() % items.len()]
    }
}

#[test]
fn generated_heterogeneous_arrays_round_trip() {
    const INTS: &[&str] = &["null", "0", "-7", "12345678901234567890123"];
    const STRS: &[&str] = &[
        "null",
        r#""""#,
        r#""a,b""#,
        r#""q\"q""#,
        r#""l\nm""#,
        r#""@0.1""#,
        r#""1e3""#,
    ];
    const KINDS: &[&str] = &["a", "b", "c", "d"];
    let mut rng = Lcg(0xb0c4e7);
    let (mut buckets, mut total) = (0, 0);
    for _ in 0..300 {
        let nrows = 1 + rng.next() % 12;
        let nkinds = 1 + rng.next() % KINDS.len();
        // Per kind: a type per column; kind `c` nests its columns.
        let items: Vec<String> = (0..nrows)
            .map(|_| {
                let k = rng.next() % nkinds;
                let mut fields = vec![format!(r#""type":"{}""#, KINDS[k])];
                for c in 0..3 {
                    let cell = if (k + c).is_multiple_of(2) {
                        rng.pick(INTS)
                    } else {
                        rng.pick(STRS)
                    };
                    if k == 2 {
                        fields.push(format!(r#""n":{{"c{c}":{cell}}}"#));
                        break;
                    } else if !rng.next().is_multiple_of(5) {
                        fields.push(format!(r#""c{c}":{cell}"#));
                    }
                }
                format!("{{{}}}", fields.join(","))
            })
            .collect();
        let array = format!("[{}]", items.join(","));
        let input = if rng.next().is_multiple_of(2) {
            array
        } else {
            format!(r#"{{"before":[1],"rows":{array},"after":"@0"}}"#)
        };
        match v2::encode(&input, &lenient()) {
            Ok(encoded) => {
                total += 1;
                buckets += usize::from(encoded.contains(" by type\n"));
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
    assert!(total > 100, "{total}");
    assert!(buckets > 50, "{buckets}");
}

fn table_ctx() -> CompressionContext<'static> {
    CompressionContext {
        mode: Mode::Safe,
        json_table: true,
        ..FIRST_SLICE_JSON_MINIFY_CONTEXT
    }
}

#[test]
fn compress_emits_bucket_blocks() {
    let input = mixed(60);
    let result = compress(&input, &table_ctx());
    let r = &result.report;
    assert_eq!(r.outcome, Outcome::Changed);
    assert_eq!(r.transform_id, JSON_TABLE_ID);
    assert_eq!(r.format_version, Some(JSON_TABLE_V2_FORMAT_VERSION));
    assert_eq!(r.validation_outcome, ValidationOutcome::Passed);
    assert!(result.output.starts_with("table-v2 @0\n@0 [60] by type\n"));
    assert_eq!(v2::decode(&result.output), Some(input.clone()));

    let wrapped = format!(r#"{{"events":{}}}"#, mixed(60));
    let result = compress(&wrapped, &table_ctx());
    assert_eq!(
        result.report.format_version,
        Some(JSON_TABLE_V2_FORMAT_VERSION)
    );
    assert!(
        result
            .output
            .starts_with("table-v2 {\"events\":@0}\n@0 [60] by type\n")
    );

    // A homogeneous bare array keeps table-v1.
    let rows: Vec<String> = (0..60)
        .map(|i| format!(r#"{{"id":{i},"t":"a"}}"#))
        .collect();
    let homogeneous = format!("[{}]", rows.join(","));
    let result = compress(&homogeneous, &table_ctx());
    assert_eq!(
        result.report.format_version,
        Some(JSON_TABLE_FORMAT_VERSION)
    );
}

#[test]
fn spec_example() {
    let text = "table-v2 @0\n@0 [5] by type\n0,1,0,2,1\n\
        @0.0 [2]{type:str,page.path:str}\nview,/a\nview,/b\n\
        @0.1 [2]{type:str,err.code:int}\nerror,500\nerror,404\n\
        @0.2 [1]{type:str,at.x:num}\nclick,1.5";
    let json = r#"[{"type":"view","page":{"path":"/a"}},{"type":"error","err":{"code":500}},{"type":"view","page":{"path":"/b"}},{"type":"click","at":{"x":1.5}},{"type":"error","err":{"code":404}}]"#;
    assert_eq!(v2::decode(text).as_deref(), Some(json));
    assert_eq!(round_trip(json), text);
}
