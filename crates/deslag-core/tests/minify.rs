use deslag_core::{JSON_MINIFY_ID, JSON_MINIFY_VERSION, minify_json};

const PRETTY: &str = r#"{
  "b": [
    1,
    -0,
    1E+10,
    1.0e-5,
    12345678901234567890123
  ],
  "a": {
    "text": "keep  these\tspaces é \ud800 \" \\ \/ \n",
    "raw": "日本語 🦀",
    "nested json": "{ \"x\" : 1 }"
  },
  "flags": [ true, false, null ],
  "empty": { },
  "list": [ ]
}
"#;

const PRETTY_MIN: &str = r#"{"b":[1,-0,1E+10,1.0e-5,12345678901234567890123],"a":{"text":"keep  these\tspaces é \ud800 \" \\ \/ \n","raw":"日本語 🦀","nested json":"{ \"x\" : 1 }"},"flags":[true,false,null],"empty":{},"list":[]}"#;

#[test]
fn identity_is_stable() {
    assert_eq!(JSON_MINIFY_ID, "json-minify");
    assert_eq!(JSON_MINIFY_VERSION, 1);
}

#[test]
fn pretty_object_is_minified_exactly() {
    assert_eq!(minify_json(PRETTY).as_deref(), Some(PRETTY_MIN));
}

#[test]
fn pretty_array_keeps_order() {
    let input = "\r\n [\n\t{ \"z\": 1, \"a\": 2 },\n\t[ 3 , 2 , 1 ]\n] \n";
    assert_eq!(
        minify_json(input).as_deref(),
        Some(r#"[{"z":1,"a":2},[3,2,1]]"#)
    );
}

#[test]
fn values_survive_a_semantic_round_trip() {
    // serde_json rejects lone surrogates, which the minifier itself copies verbatim.
    let pretty = PRETTY.replace(" \\ud800", "");
    let fixtures = [
        pretty.as_str(),
        "[ 1, 2.50, \"x y\" ]",
        "{ \"k\": [ { } ] }",
    ];
    for input in fixtures {
        let out = minify_json(input).expect(input);
        let a: serde_json::Value = serde_json::from_str(input).unwrap();
        let b: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(a, b, "{input}");
    }
}

#[test]
fn ineligible_input_passes_through() {
    let deep_ok = format!("{}{}\n", "[".repeat(128), "]".repeat(128));
    assert!(minify_json(&deep_ok).is_some());
    let too_deep = format!("{}{}\n", "[".repeat(129), "]".repeat(129));
    for input in [
        r#"{"already":"minified"}"#,
        "[1,2]",
        "{ not json }",
        "{ \"a\": [1, 2",
        "{ \"a\": 1 } x",
        "{ \"a\": 1 }\n{ \"b\": 2 }",
        "42\n",
        " \"str\" ",
        "true ",
        "null",
        "",
        "   ",
        "{ \"a\": 1, \"a\": 2 }",
        "{ \"a\\u0062\": 1 }",
        "{ \"a\": \"x\u{1}y\" }",
        "{ \"a\": \"x\ty\" }",
        "{ \"a\": \"\\x\" }",
        "{ \"a\": \"\\u12G4\" }",
        "{ \"a\": 01 }",
        "{ \"a\": 1. }",
        "{ \"a\": .5 }",
        "{ \"a\": 1e }",
        "{ \"a\": +1 }",
        "{ \"a\": tru }",
        "[ 1, ]",
        "{ \"a\": 1, }",
        "[\u{a0}1]",
        &too_deep,
    ] {
        assert_eq!(minify_json(input), None, "{input:?}");
    }
}

#[test]
fn duplicate_keys_count_per_object_only() {
    let input = "[ { \"a\": 1 }, { \"a\": 2, \"b\": { \"a\": 3 } } ]";
    assert_eq!(
        minify_json(input).as_deref(),
        Some(r#"[{"a":1},{"a":2,"b":{"a":3}}]"#)
    );
}

#[test]
fn json_inside_strings_is_not_recursed() {
    let input = "[ \"{ \\\"a\\\" : 1 }\" ]";
    assert_eq!(
        minify_json(input).as_deref(),
        Some("[\"{ \\\"a\\\" : 1 }\"]")
    );
}

#[test]
fn minification_is_idempotent() {
    let once = minify_json(PRETTY).unwrap();
    assert_eq!(minify_json(&once), None);
}
