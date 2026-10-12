use deslag_core::{
    CompressionContext, FIRST_SLICE_JSON_MINIFY_CONTEXT, JSON_TABLE_ID,
    JSON_TABLE_V2_FORMAT_VERSION, JSON_TABLE_VERSION, Mode, Outcome, TableOptions,
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

/// The canonical JSON string lexeme for `text`.
fn lexeme(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}

/// `n` rows of `{"k":<i>,"v":"<word>"}` as minified JSON.
fn inner(n: usize) -> String {
    let items: Vec<String> = (0..n)
        .map(|i| format!(r#"{{"k":{i},"v":"w{i}"}}"#))
        .collect();
    format!("[{}]", items.join(","))
}

/// `n` rows whose `data` cell is the string lexeme produced by `cell(i)`.
fn rows_with(n: usize, cell: impl Fn(usize) -> String) -> String {
    let items: Vec<String> = (0..n)
        .map(|i| format!(r#"{{"id":{i},"data":{}}}"#, cell(i)))
        .collect();
    format!("[{}]", items.join(","))
}

#[test]
fn identity_is_bumped() {
    assert_eq!(JSON_TABLE_VERSION, 4);
}

#[test]
fn string_holding_an_array_of_objects() {
    let input = rows_with(3, |_| lexeme(&inner(4)));
    let encoded = round_trip(&input);
    assert!(
        encoded.starts_with(
            "table-v2 @0\n@0 [3]{id:int,data:json}\n0,@0\n1,@1\n2,@2\n@0.0 [4]{k:int,v:str}\n0,w0\n"
        ),
        "{encoded}"
    );
    assert!(encoded.contains("\n@0.2 [4]{k:int,v:str}\n"), "{encoded}");
    // table-v1 never emits a json column.
    let v1 = table::encode(&input, &lenient()).unwrap();
    assert!(v1.starts_with("table-v1 [3]{id:int,data:str}"), "{v1}");
}

#[test]
fn string_holding_an_object_that_wraps_arrays() {
    let doc = format!(r#"{{"page":1,"items":{},"more":{}}}"#, inner(4), inner(3));
    let input = rows_with(2, |_| lexeme(&doc));
    let encoded = round_trip(&input);
    assert!(
        encoded
            .contains("\n@0.0 = {\"page\":1,\"items\":@0,\"more\":@1}\n@0.0.0 [4]{k:int,v:str}\n"),
        "{encoded}"
    );
    assert!(encoded.contains("\n@0.1.1 [3]{k:int,v:str}\n"), "{encoded}");
}

/// `level` 0 is `inner(3)`; each further level is three rows whose `data`
/// cell holds the previous level as a string.
fn nested(level: usize) -> String {
    if level == 0 {
        return inner(3);
    }
    rows_with(3, |_| lexeme(&nested(level - 1)))
}

#[test]
fn nested_sub_tables_stop_at_the_bound() {
    let encoded = round_trip(&nested(5));
    // Four nested levels of sub-blocks; the strings in the fifth stay `str`.
    assert!(
        encoded.contains("\n@0.0.0.0.0 [3]{id:int,data:str}\n"),
        "{encoded}"
    );
    assert!(!encoded.contains("\n@0.0.0.0.0.0 "), "{encoded}");
    assert_eq!(
        encoded.matches("data:json").count(),
        1 + 3 + 9 + 27,
        "{encoded}"
    );
    // Within the bound every level nests.
    let encoded = round_trip(&nested(4));
    assert!(!encoded.contains("data:str"), "{encoded}");
}

#[test]
fn json_lookalikes_stay_strings() {
    let cases = [
        // Not minified.
        lexeme(r#"[{"k": 1}, {"k": 2}, {"k": 3}]"#),
        // Inner strings with escapes the table encoder cannot restore.
        lexeme(r#"[{"k":"a\/b"},{"k":"A"},{"k":"c"}]"#),
        // Invalid or truncated JSON.
        lexeme(r#"[{"k":1},{"k":2},{"k":3}"#),
        lexeme(r#"[{"k":1},{"k":2}]x"#),
        // Scalars and arrays of scalars.
        lexeme("12345678901234567890"),
        lexeme(r#""a quoted string""#),
        lexeme("[1,2,3,4,5,6,7,8,9,10,11,12]"),
        // Duplicate keys.
        lexeme(r#"[{"k":1,"k":2},{"k":3,"k":4}]"#),
    ];
    for cell in cases {
        let input = rows_with(3, |_| cell.clone());
        let encoded = round_trip(&input);
        assert!(!encoded.contains(":json"), "{encoded}");
    }
}

#[test]
fn non_canonical_outer_lexemes_are_not_tables() {
    // `\/` in the outer string has no canonical spelling, so no table applies.
    let input = rows_with(3, |_| r#""[{\"k\":1},{\"k\":2}]\/""#.to_string());
    assert!(v2::encode(&input, &lenient()).is_err());
}

#[test]
fn reference_lookalikes_are_quoted_in_json_columns() {
    let input = rows_with(6, |i| match i {
        1 => lexeme("@0"),
        3 => lexeme("@12"),
        4 => lexeme("@x"),
        _ => lexeme(&inner(4)),
    });
    let encoded = round_trip(&input);
    assert!(
        encoded.contains("{id:int,data:json}\n0,@0\n1,\"@0\"\n2,@1\n3,\"@12\"\n4,@x\n5,@2\n"),
        "{encoded}"
    );
    // In a str column a bare `@n` cell stays string text.
    let input = rows_with(3, |i| lexeme(&format!("@{i}")));
    assert!(round_trip(&input).contains("{id:int,data:str}\n0,@0\n1,@1\n2,@2"));
}

#[test]
fn mixed_rows_convert_only_json_cells() {
    let items = [
        format!(r#"{{"id":0,"data":{},"note":"plain"}}"#, lexeme(&inner(4))),
        r#"{"id":1,"data":null,"note":"[1,2]"}"#.to_string(),
        r#"{"id":2,"note":"x"}"#.to_string(),
        r#"{"id":3,"data":"short","note":"y"}"#.to_string(),
        format!(r#"{{"id":4,"data":{},"note":"z"}}"#, lexeme(&inner(5))),
        r#"{"id":5,"data":"[{\"k\":1}]","note":"z"}"#.to_string(),
    ];
    let input = format!("[{}]", items.join(","));
    let encoded = round_trip(&input);
    assert!(
        encoded.contains(
            "{id:int,data:json,note:str}\n0,@0,plain\n1,null,\"[1,2]\"\n2,,x\n3,short,y\n4,@1,z\n5,\"[{\"\"k\"\":1}]\",z\n@0.0 [4]"
        ),
        "{encoded}"
    );
}

#[test]
fn wrapped_arrays_and_bucket_blocks_hold_sub_tables() {
    let wrapped = format!(
        r#"{{"total":3,"results":{}}}"#,
        rows_with(3, |_| lexeme(&inner(4)))
    );
    let encoded = round_trip(&wrapped);
    assert!(
        encoded.starts_with("table-v2 {\"total\":3,\"results\":@0}\n@0 [3]{id:int,data:json}\n"),
        "{encoded}"
    );

    // Kinds `a` and `b` disagree on the type of `n`, so they become buckets.
    let items: Vec<String> = (0..6)
        .map(|i| {
            let n = if i % 2 == 0 {
                format!("{i}")
            } else {
                format!("\"s{i}\"")
            };
            format!(
                r#"{{"type":"{}","n":{n},"data":{}}}"#,
                if i % 2 == 0 { "a" } else { "b" },
                lexeme(&inner(4))
            )
        })
        .collect();
    let bare = format!("[{}]", items.join(","));
    let encoded = round_trip(&bare);
    assert!(
        encoded.starts_with(
            "table-v2 @0\n@0 [6] by type\n0,1,0,1,0,1\n@0.0 [3]{type:str,n:int,data:json}\na,0,@0\n"
        ),
        "{encoded}"
    );
    assert!(encoded.contains("\n@0.0.2 [4]"), "{encoded}");
    assert!(
        encoded.contains("\n@0.1 [3]{type:str,n:str,data:json}\n"),
        "{encoded}"
    );
    assert!(encoded.contains("\n@0.1.2 [4]"), "{encoded}");
    let wrapped = format!(r#"{{"events":{bare}}}"#);
    assert!(round_trip(&wrapped).contains("\n@0.1.0 [4]"));
}

#[test]
fn decoder_accepts_hand_written_sub_tables() {
    assert_eq!(
        v2::decode("table-v2 @0\n@0 [3]{id:int,t:json}\n1,@0\n2,\"@1\"\n3,@x\n@0.0 [1]{a:int}\n5")
            .as_deref(),
        Some(r#"[{"id":1,"t":"[{\"a\":5}]"},{"id":2,"t":"@1"},{"id":3,"t":"@x"}]"#)
    );
    assert_eq!(
        v2::decode("table-v2 {\"r\":@0}\n@0 [1]{t:json}\n@0\n@0.0 = {\"x\":@0,\"y\":1}\n@0.0.0 [1]{a:str}\n\"\"\"\"")
            .as_deref(),
        Some(r#"{"r":[{"t":"{\"x\":[{\"a\":\"\\\"\"}],\"y\":1}"}]}"#)
    );
    // A json column with no reference cell is valid, though never emitted.
    assert_eq!(
        v2::decode("table-v2 @0\n@0 [1]{t:json}\nx").as_deref(),
        Some(r#"[{"t":"x"}]"#)
    );
}

#[test]
fn decoder_rejects_malformed_sub_tables() {
    let sub = "@0.0 [1]{a:int}\n5";
    for text in [
        // table-v1 has no json column.
        "table-v1 [1]{t:json}\nx".to_string(),
        // Reference out of order, padded or with a leading zero.
        format!("table-v2 @0\n@0 [1]{{t:json}}\n@1\n{sub}"),
        format!("table-v2 @0\n@0 [1]{{t:json}}\n@00\n{sub}"),
        // Missing, misnumbered, or extra sub-block.
        "table-v2 @0\n@0 [1]{t:json}\n@0".to_string(),
        "table-v2 @0\n@0 [1]{t:json}\n@0\n@0.1 [1]{a:int}\n5".to_string(),
        "table-v2 @0\n@0 [1]{t:json}\n@0\n@1 [1]{a:int}\n5".to_string(),
        format!("table-v2 @0\n@0 [1]{{t:json}}\nx\n{sub}"),
        format!("table-v2 @0\n@0 [1]{{t:json}}\n@0\n{sub}\n@0.1 [1]{{a:int}}\n5"),
        // A malformed sub-block.
        "table-v2 @0\n@0 [1]{t:json}\n@0\n@0.0 [2]{a:int}\n5".to_string(),
        "table-v2 @0\n@0 [1]{t:json}\n@0\n@0.0 x".to_string(),
        // A skeleton block with no hole, an invalid skeleton, or a nested one.
        "table-v2 @0\n@0 [1]{t:json}\n@0\n@0.0 = {\"a\":1}".to_string(),
        "table-v2 @0\n@0 [1]{t:json}\n@0\n@0.0 = {@0:1}\n@0.0.0 [1]{a:int}\n5".to_string(),
        "table-v2 @0\n@0 [1]{t:json}\n@0\n@0.0 = {\"a\":@0}\n@0.0.0 = [@0]\n@0.0.0.0 [1]{a:int}\n5"
            .to_string(),
        // A skeleton block is only a sub-block, never a top-level block.
        "table-v2 @0\n@0 = [@0]\n@0.0 [1]{a:int}\n5".to_string(),
        // Text after the last sub-block.
        format!("table-v2 @0\n@0 [1]{{t:json}}\n@0\n{sub}\n"),
    ] {
        assert_eq!(v2::decode(&text), None, "{text}");
    }
    assert_eq!(table::decode("table-v1 [1]{t:json}\nx"), None);
}

#[test]
fn decoder_rejects_nesting_beyond_the_bound() {
    // Five levels of sub-blocks under the top-level block.
    let mut text = String::from("table-v2 @0\n@0 [1]{t:json}\n@0");
    let mut label = String::from("@0");
    for _ in 0..4 {
        label.push_str(".0");
        text.push_str(&format!("\n{label} [1]{{t:json}}\n@0"));
    }
    let leaf = format!("{text}\n{label}.0 [1]{{a:int}}\n5");
    assert_eq!(v2::decode(&leaf), None);
    // Four levels decode.
    let (head, _) = text.rsplit_once(&format!("\n{label} ")).unwrap();
    let four = format!("{head}\n{label} [1]{{a:int}}\n5");
    assert!(v2::decode(&four).is_some(), "{four}");
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
}

/// A random JSON value as minified text: either a scalar, a JSON-looking
/// string, an array of rows or a wrapper object, nested up to `depth`.
fn gen_value(rng: &mut Lcg, depth: usize) -> String {
    match rng.next() % if depth == 0 { 4 } else { 7 } {
        0 => "null".to_string(),
        1 => format!("{}", rng.next() % 100),
        2 => lexeme(["@0", "a,b", "[", "{\"x\":1}", "q\"q"][rng.next() % 5]),
        3 => lexeme(&format!("[{{\"k\": {}}}]", rng.next() % 9)),
        4 => lexeme(&gen_rows(rng, depth - 1)),
        5 => lexeme(&format!(
            r#"{{"n":{},"r":{}}}"#,
            rng.next() % 9,
            gen_rows(rng, depth - 1)
        )),
        _ => gen_rows(rng, depth - 1),
    }
}

fn gen_rows(rng: &mut Lcg, depth: usize) -> String {
    let n = 1 + rng.next() % 5;
    let items: Vec<String> = (0..n)
        .map(|i| {
            let kind = ["a", "b"][rng.next() % 2];
            format!(
                r#"{{"type":"{kind}","i":{i},"v":{}}}"#,
                gen_value(rng, depth)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

#[test]
fn generated_inputs_round_trip() {
    let mut rng = Lcg(43);
    let (mut encoded_count, mut with_json) = (0, 0);
    for case in 0..400 {
        let rows = gen_rows(&mut rng, 3);
        let input = if case % 2 == 0 {
            rows
        } else {
            format!(r#"{{"total":{case},"rows":{rows}}}"#)
        };
        if let Ok(encoded) = v2::encode(&input, &lenient()) {
            encoded_count += 1;
            with_json += usize::from(encoded.contains(":json"));
            assert_eq!(v2::decode(&encoded), Some(input.clone()), "{encoded}");
        }
    }
    assert!(encoded_count > 100, "{encoded_count}");
    assert!(with_json > 30, "{with_json}");
}

#[test]
fn compress_emits_sub_tables() {
    let input = format!(
        "{{\n  \"results\": {},\n  \"total\": 40\n}}",
        rows_with(40, |_| lexeme(&inner(6)))
    );
    let ctx = CompressionContext {
        mode: Mode::Safe,
        json_table: true,
        ..FIRST_SLICE_JSON_MINIFY_CONTEXT
    };
    let result = compress(&input, &ctx);
    let r = &result.report;
    assert_eq!(r.outcome, Outcome::Changed);
    assert_eq!(r.transform_id, JSON_TABLE_ID);
    assert_eq!(r.transform_version, JSON_TABLE_VERSION);
    assert_eq!(r.format_version, Some(JSON_TABLE_V2_FORMAT_VERSION));
    assert_eq!(r.validation_outcome, ValidationOutcome::Passed);
    assert!(
        result.output.contains("{id:int,data:json}"),
        "{}",
        result.output
    );
    assert_eq!(v2::decode(&result.output), Some(minified(&input)));
}

#[test]
fn spec_example() {
    let text =
        "table-v2 @0\n@0 [2]{id:int,data:json}\n1,@0\n2,\"@1\"\n@0.0 [2]{k:int,v:str}\n0,a\n1,b";
    let json =
        r#"[{"id":1,"data":"[{\"k\":0,\"v\":\"a\"},{\"k\":1,\"v\":\"b\"}]"},{"id":2,"data":"@1"}]"#;
    assert_eq!(v2::decode(text).as_deref(), Some(json));
}

#[test]
fn sub_tables_never_lengthen_a_block() {
    // One cell pays as a reference, but the forty `@n` look-alikes would each
    // gain two quote bytes in a `json` column, so the plain block is kept.
    let rows = rows_with(41, |i| match i {
        0 => lexeme(&inner(4)),
        _ => lexeme(&format!("@{i}")),
    });
    let encoded = round_trip(&format!(r#"{{"rows":{rows}}}"#));
    assert!(
        encoded.starts_with("table-v2 {\"rows\":@0}\n@0 [41]{id:int,data:str}\n"),
        "{encoded}"
    );
    // Without the look-alikes the same cell does become a reference.
    let rows = rows_with(2, |_| lexeme(&inner(4)));
    let encoded = round_trip(&format!(r#"{{"rows":{rows}}}"#));
    assert!(encoded.contains("{id:int,data:json}"), "{encoded}");
}
