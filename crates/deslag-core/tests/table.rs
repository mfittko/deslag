use deslag_core::{
    JSON_TABLE_FORMAT_VERSION, JSON_TABLE_ID, JSON_TABLE_VERSION, Reason, TableOptions, TokenCount,
    TokenCounter, TokenMethod, minify_json,
    table::{MARKER, decode, encode},
};

/// No byte-ratio threshold, so small fixtures only need to be shorter.
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
    let encoded = encode(input, &lenient()).unwrap_or_else(|r| panic!("{r:?}: {input}"));
    assert_eq!(decode(&encoded), Some(minified(input)), "{encoded}");
    encoded
}

/// `[rows,rows,...]` with `rows` repeated `n` times.
fn repeat(rows: &str, n: usize) -> String {
    format!("[{}]", vec![rows; n].join(","))
}

#[test]
fn identity_is_stable() {
    assert_eq!(JSON_TABLE_ID, "json-table");
    assert_eq!(JSON_TABLE_VERSION, 1);
    assert_eq!(JSON_TABLE_FORMAT_VERSION, 1);
    assert_eq!(MARKER, "table-v1");
}

#[test]
fn encodes_header_and_rows() {
    let input = r#"[
      {"id": 1, "name": "alpha", "score": 1.5, "active": true},
      {"id": 2, "name": "beta",  "score": 2,   "active": false},
      {"id": 3, "name": "gamma", "score": null, "active": true}
    ]"#;
    assert_eq!(
        round_trip(input),
        "table-v1 [3]{id:int,name:str,score:num,active:bool}\n\
         1,alpha,1.5,true\n2,beta,2,false\n3,gamma,null,true"
    );
}

#[test]
fn round_trip_corpus() {
    let corpus = [
        repeat(r#"{"value":1.0,"other":-0},{"value":1e3,"other":0}"#, 3),
        repeat(
            r#"{"big":12345678901234567890123,"exp":1E+10},{"big":-98765432109876543210,"exp":1.0e-5}"#,
            3,
        ),
        repeat(r#"{"a":1,"b":2,"c":3},{"a":4,"c":6},{"b":8}"#, 3),
        repeat(
            r#"{"first":"x"},{"second":"y"},{"first":"z","second":"w"}"#,
            3,
        ),
        repeat(
            r#"{"text":"tab\there \"q\" back\\slash é 日本語 🦀 / \u0001 \b\f\r\n"},{"text":"plain"}"#,
            3,
        ),
        repeat(
            r#"{"nulls":null,"flag":true},{"nulls":null,"flag":null}"#,
            3,
        ),
        repeat(r#"{"key:with:colons":1,"key with space":2,"{}[]":3}"#, 3),
        repeat(r#"{"":"empty key","k":1},{"":"again","k":2}"#, 3),
    ];
    for input in &corpus {
        round_trip(input);
        let pretty = input.replace(',', " ,\n  ").replace(':', " : ");
        if serde_json::from_str::<serde_json::Value>(&pretty).is_ok() {
            round_trip(&pretty);
        }
    }
}

#[test]
fn row_and_field_order_survive() {
    let input = repeat(r#"{"z":1,"m":2,"a":3},{"z":4,"a":6},{"m":8}"#, 2);
    let encoded = round_trip(&input);
    assert!(encoded.starts_with("table-v1 [6]{z:int,m:int,a:int}\n1,2,3\n4,,6\n,8,\n"));
}

#[test]
fn null_missing_and_empty_string_stay_distinct() {
    let input = r#"[{"alpha":null,"beta":"b"},{"beta":"c"},{"alpha":"","beta":"d"},{"alpha":"x","beta":"e"}]"#;
    let encoded = round_trip(input);
    assert_eq!(
        encoded,
        "table-v1 [4]{alpha:str,beta:str}\nnull,b\n,c\n\"\",d\nx,e"
    );
}

#[test]
fn ambiguous_strings_are_quoted() {
    let cases = [
        ("null", r#""null""#),
        ("true", r#""true""#),
        ("false", r#""false""#),
        ("1", r#""1""#),
        ("-0", r#""-0""#),
        ("1e3", r#""1e3""#),
        ("1.0", r#""1.0""#),
        ("NaN", r#""NaN""#),
        ("", r#""""#),
        ("a,b", r#""a,b""#),
        (r#"say \"hi\""#, r#""say ""hi""""#),
        (r"line\nbreak", "\"line\nbreak\""),
        (r"cr\rhere", "\"cr\rhere\""),
        ("plain text", "plain text"),
    ];
    for (json_text, cell) in cases {
        let input = format!(
            r#"[{{"identifier":1,"text":"{json_text}"}},{{"identifier":2,"text":"{json_text}"}}]"#
        );
        let encoded = round_trip(&input);
        let expected = format!("table-v1 [2]{{identifier:int,text:str}}\n1,{cell}\n2,{cell}");
        assert_eq!(encoded, expected);
    }
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
fn generated_tables_round_trip() {
    const INTS: &[&str] = &["0", "-0", "7", "-42", "12345678901234567890123"];
    const NUMS: &[&str] = &["1.0", "-0.5", "1e3", "2E-7", "3", "1.5e+300"];
    // Bodies of canonical JSON string lexemes.
    const STRS: &[&str] = &[
        "", "null", "true", "false", "1", "-0", "1e3", "a,b", r#"q\"q"#, r"a\nb", r"\t", r"\\",
        r"\u001f", "x y", " lead", "é🦀", "/", "plain", ",", r#"\""#,
    ];
    let mut rng = Lcg(0x5eed);
    let mut encoded_count = 0;
    for _ in 0..300 {
        let ncols = 1 + rng.next() as usize % 5;
        let types: Vec<u64> = (0..ncols).map(|_| rng.next() % 4).collect();
        let nrows = 2 + rng.next() as usize % 8;
        let mut rows = Vec::new();
        for _ in 0..nrows {
            let mut fields = Vec::new();
            for (c, ty) in types.iter().enumerate() {
                let value = match rng.next() % 8 {
                    0 => continue,
                    1 => "null".to_string(),
                    _ => match ty {
                        0 => rng.pick(INTS).to_string(),
                        1 => rng.pick(NUMS).to_string(),
                        2 => rng.pick(&["true", "false"]).to_string(),
                        _ => format!("\"{}\"", rng.pick(STRS)),
                    },
                };
                fields.push(format!("\"col{c}\":{value}"));
            }
            rows.push(format!("{{{}}}", fields.join(",")));
        }
        let input = format!("[{}]", rows.join(","));
        let opts = TableOptions {
            min_savings_permille: 0,
            ..TableOptions::default()
        };
        match encode(&input, &opts) {
            Ok(encoded) => {
                encoded_count += 1;
                assert_eq!(
                    decode(&encoded).as_deref(),
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
    assert!(encoded_count > 150, "{encoded_count}");
}

fn reject(input: &str, opts: &TableOptions, reason: Reason) {
    assert_eq!(encode(input, opts), Err(reason), "{input}");
}

fn shape(input: &str) {
    reject(input, &lenient(), Reason::UnsupportedFormat);
}

#[test]
fn ineligible_not_an_array() {
    shape(r#"{"rows":[{"a":1},{"a":2}]}"#);
    shape("not json");
}

#[test]
fn ineligible_fewer_than_two_rows() {
    shape(r#"[{"identifier":1,"name":"alpha"}]"#);
    shape("[]");
}

#[test]
fn ineligible_non_object_item() {
    shape(&repeat(r#"{"identifier":1},2"#, 4));
}

#[test]
fn ineligible_array_value() {
    shape(&repeat(r#"{"identifier":[1]},{"identifier":[2]}"#, 4));
}

#[test]
fn ineligible_object_value() {
    shape(&repeat(r#"{"meta":{"r":1}},{"meta":{"r":2}}"#, 4));
}

#[test]
fn ineligible_mixed_types() {
    shape(&repeat(r#"{"value":1},{"value":"1"}"#, 4));
    shape(&repeat(r#"{"value":true},{"value":0}"#, 4));
}

#[test]
fn int_and_num_mix_to_num() {
    let encoded = round_trip(&repeat(r#"{"value":1},{"value":1.5}"#, 4));
    assert!(encoded.starts_with("table-v1 [8]{value:num}\n"));
}

#[test]
fn ineligible_bad_key_characters() {
    shape(&repeat(r#"{"a.b":1},{"a.b":2}"#, 4));
    shape(&repeat(r#"{"a,b":1},{"a,b":2}"#, 4));
    // A key with `"`, `\n` or `\r` needs an escape, which the scanner rejects.
    reject(
        &repeat(r#"{"a\"b":1},{"a\"b":2}"#, 4),
        &lenient(),
        Reason::EscapedKey,
    );
}

#[test]
fn ineligible_duplicate_keys() {
    reject(
        &repeat(r#"{"value":1,"value":2},{"value":3}"#, 4),
        &lenient(),
        Reason::DuplicateKey,
    );
}

#[test]
fn ineligible_key_order_mismatch() {
    shape(&repeat(r#"{"alpha":1,"beta":2},{"beta":3,"alpha":4}"#, 4));
}

#[test]
fn ineligible_no_columns() {
    shape("[{},{},{}]");
}

#[test]
fn ineligible_too_many_rows() {
    let input = repeat(r#"{"identifier":1}"#, 5);
    let opts = TableOptions {
        max_rows: 4,
        ..lenient()
    };
    reject(&input, &opts, Reason::UnsupportedFormat);
    assert!(
        encode(
            &input,
            &TableOptions {
                max_rows: 5,
                ..opts
            }
        )
        .is_ok()
    );
}

#[test]
fn ineligible_already_encoded() {
    let encoded = round_trip(&repeat(r#"{"identifier":1}"#, 5));
    shape(&encoded);
    shape(&format!(" \n{encoded}"));
}

#[test]
fn ineligible_non_canonical_string_lexeme() {
    // Built at run time so no `\u` escape for a printable character sits in source.
    let u = |hex: &str| format!("{}u{hex}", '\\');
    let surrogates = format!("{}{}", u("d83e"), u("dd80"));
    for body in [r"\/", &u("0041"), &u("001F"), &surrogates, &u("00e9")] {
        shape(&repeat(
            &format!(r#"{{"text":"{body}"}},{{"text":"x"}}"#),
            4,
        ));
    }
}

#[test]
fn ineligible_depth_limit() {
    let opts = TableOptions {
        max_depth: 1,
        ..lenient()
    };
    reject(&repeat(r#"{"identifier":1}"#, 4), &opts, Reason::DepthLimit);
}

#[test]
fn gain_requires_shorter_output() {
    // Encoded `table-v1 [2]{a:int}\n1\n2` is longer than `[{"a":1},{"a":2}]`.
    reject(r#"[{"a":1},{"a":2}]"#, &lenient(), Reason::InsufficientGain);
}

#[test]
fn gain_byte_threshold_is_inclusive() {
    let input = repeat(r#"{"identifier":12,"name":"alpha"}"#, 10);
    let (m, e) = (input.len() as u64, round_trip(&input).len() as u64);
    let permille = (m - e) * 1000 / m;
    let at = |p| TableOptions {
        min_savings_permille: p,
        ..TableOptions::default()
    };
    assert!(encode(&input, &at(permille)).is_ok());
    reject(&input, &at(permille + 1), Reason::InsufficientGain);
    // The default 10% threshold applies to the minified original.
    reject(
        &repeat(r#"{"a":1}"#, 2),
        &TableOptions::default(),
        Reason::InsufficientGain,
    );
}

#[test]
fn gain_check_does_not_overflow() {
    let opts = TableOptions {
        min_savings_permille: u64::MAX / 10,
        ..lenient()
    };
    let input = repeat(r#"{"identifier":12,"name":"alpha"}"#, 10);
    reject(&input, &opts, Reason::InsufficientGain);
}

/// One token per byte, so savings are easy to compute.
struct Bytes;

impl TokenCounter for Bytes {
    fn count(&self, text: &str) -> TokenCount {
        TokenCount {
            count: text.len() as u64,
            method: TokenMethod::Estimate,
            tokenizer: "bytes",
        }
    }
}

#[test]
fn gain_token_threshold_applies_with_counter() {
    let input = repeat(r#"{"identifier":12,"name":"alpha"}"#, 10);
    let saved = (minified(&input).len() - round_trip(&input).len()) as u64;
    let with = |t| TableOptions {
        min_token_savings: t,
        token_counter: Some(&Bytes),
        ..lenient()
    };
    assert!(encode(&input, &with(saved)).is_ok());
    reject(&input, &with(saved + 1), Reason::InsufficientGain);
    // Without a counter the token threshold is ignored.
    let no_counter = TableOptions {
        token_counter: None,
        ..with(u64::MAX)
    };
    assert!(encode(&input, &no_counter).is_ok());
}

/// Counts the encoded table as more tokens than any minified JSON.
struct TableCostsMore;

impl TokenCounter for TableCostsMore {
    fn count(&self, text: &str) -> TokenCount {
        TokenCount {
            count: u64::from(text.starts_with(MARKER)),
            method: TokenMethod::Estimate,
            tokenizer: "table-costs-more",
        }
    }
}

#[test]
fn gain_token_regression_fails_at_zero_threshold() {
    let input = repeat(r#"{"identifier":12,"name":"alpha"}"#, 10);
    let opts = TableOptions {
        min_token_savings: 0,
        token_counter: Some(&TableCostsMore),
        ..lenient()
    };
    reject(&input, &opts, Reason::InsufficientGain);
}

#[test]
fn decoder_accepts_zero_rows() {
    assert_eq!(decode("table-v1 [0]{a:str}").as_deref(), Some("[]"));
}

#[test]
fn decoder_rejects_malformed_text() {
    let bad = [
        "",
        "table-v2 [1]{a:int}\n1",
        "table-v1 []{a:int}\n1",
        "table-v1 [+1]{a:int}\n1",
        "table-v1 [01]{a:int}\n1",
        "table-v1 [00]{a:int}",
        "table-v1 [1]{}\n1",
        "table-v1 [1]{a}\n1",
        "table-v1 [1]{a:float}\n1",
        "table-v1 [1]{a:int,a:int}\n1,2",
        "table-v1 [1]{a.b:int}\n1",
        "table-v1 [1]{a:int\n1",
        "table-v1 [2]{a:int}\n1",
        "table-v1 [1]{a:int}\n1\n2",
        "table-v1 [1]{a:int}\n1\n",
        "table-v1 [1]{a:int}",
        "table-v1 [0]{a:int}\n",
        "table-v1 [1]{a:int,b:int}\n1",
        "table-v1 [1]{a:int}\n1,2",
        "table-v1 [1]{a:int}\n\"1\"",
        "table-v1 [1]{a:int}\n1.5",
        "table-v1 [1]{a:int}\n01",
        "table-v1 [1]{a:num}\nabc",
        "table-v1 [1]{a:num}\n.5",
        "table-v1 [1]{a:bool}\nyes",
        "table-v1 [1]{a:str}\n\"open",
        "table-v1 [1]{a:str}\n\"a\"b",
        "table-v1 [1]{a:str}\na\"b",
        "table-v1 [1]{a:str}\na\rb",
    ];
    for text in bad {
        assert_eq!(decode(text), None, "{text:?}");
    }
}

#[test]
fn decoder_escapes_strings_canonically() {
    assert_eq!(
        decode("table-v1 [1]{a:str,b:str}\n\"x\ny\"\"\\\t\u{1}\",é").as_deref(),
        Some(r#"[{"a":"x\ny\"\\\t\u0001","b":"é"}]"#)
    );
}
