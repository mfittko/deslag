//! Property tests over random valid JSON from a fixed-seed generator.

use std::borrow::Cow;

use deslag_core::{FIRST_SLICE_JSON_MINIFY_CONTEXT, Outcome, Reason, compress, minify_json};

/// xorshift64*: small, deterministic and dependency-free.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

enum Json {
    Object(Vec<(String, Json)>),
    Array(Vec<Json>),
    /// A string, number or literal lexeme.
    Scalar(String),
}

/// String pieces: raw text, every escape, escaped Unicode and embedded JSON.
/// Lone surrogates are left out because serde_json rejects them.
const PIECES: &[&str] = &[
    "a",
    "Z",
    " ",
    "  ",
    "é",
    "日本",
    "🦀",
    "\u{2028}",
    "\\\"",
    "\\\\",
    "\\/",
    "\\b",
    "\\f",
    "\\n",
    "\\r",
    "\\t",
    "\\u00e9",
    "\\u00E9",
    "\\uD83E\\uDD80",
    "\\u0000",
    "{ \\\"x\\\" : [ 1 , 2 ] }",
];

fn string(rng: &mut Rng) -> String {
    let mut s = String::from("\"");
    for _ in 0..rng.below(5) {
        s.push_str(rng.pick(PIECES));
    }
    s.push('"');
    s
}

fn digits(rng: &mut Rng, max: usize) -> String {
    (0..=rng.below(max))
        .map(|_| char::from(b'0' + rng.below(10) as u8))
        .collect()
}

/// Covers -0, fractions, every exponent form and integers beyond u64.
fn number(rng: &mut Rng) -> String {
    let mut n = String::from(rng.pick(&["", "-"]));
    if rng.below(3) == 0 {
        n.push('0');
    } else {
        n.push(char::from(b'1' + rng.below(9) as u8));
        n.push_str(&digits(rng, 30));
    }
    if rng.below(2) == 0 {
        n.push('.');
        n.push_str(&digits(rng, 20));
    }
    if rng.below(2) == 0 {
        n.push_str(rng.pick(&["e", "E"]));
        n.push_str(rng.pick(&["", "+", "-"]));
        // Two digits at most, so serde_json stays within f64 range.
        n.push_str(&digits(rng, 2));
    }
    n
}

fn value(rng: &mut Rng, depth: usize) -> Json {
    let kinds = if depth == 0 { 3 } else { 5 };
    match rng.below(kinds) {
        0 => Json::Scalar(string(rng)),
        1 => Json::Scalar(number(rng)),
        2 => Json::Scalar(rng.pick(&["true", "false", "null"]).to_string()),
        k => container(rng, depth - 1, k == 3),
    }
}

fn container(rng: &mut Rng, depth: usize, object: bool) -> Json {
    let len = rng.below(5);
    if object {
        // The index keeps keys unique; the suffix has no quote or backslash.
        let members = (0..len)
            .map(|i| {
                let key = format!("\"k{i}{}\"", rng.pick(&["", " x", "é", "🦀"]));
                (key, value(rng, depth))
            })
            .collect();
        Json::Object(members)
    } else {
        Json::Array((0..len).map(|_| value(rng, depth)).collect())
    }
}

fn ws(rng: &mut Option<&mut Rng>, out: &mut String) {
    if let Some(rng) = rng.as_deref_mut() {
        out.push_str(rng.pick(&["", "", "", " ", "\n", "\t", "\r\n", "\n    "]));
    }
}

/// Renders `json`; with an `rng`, adds random insignificant whitespace.
fn render(json: &Json, rng: &mut Option<&mut Rng>, out: &mut String) {
    match json {
        Json::Scalar(s) => out.push_str(s),
        Json::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                ws(rng, out);
                if i > 0 {
                    out.push(',');
                    ws(rng, out);
                }
                render(item, rng, out);
            }
            ws(rng, out);
            out.push(']');
        }
        Json::Object(members) => {
            out.push('{');
            for (i, (key, item)) in members.iter().enumerate() {
                ws(rng, out);
                if i > 0 {
                    out.push(',');
                    ws(rng, out);
                }
                out.push_str(key);
                ws(rng, out);
                out.push(':');
                ws(rng, out);
                render(item, rng, out);
            }
            ws(rng, out);
            out.push('}');
        }
    }
}

#[test]
fn random_valid_json_minifies_exactly() {
    let ctx = FIRST_SLICE_JSON_MINIFY_CONTEXT;
    for seed in 1..=400u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let object = rng.below(2) == 0;
        let doc = container(&mut rng, 4, object);
        let mut compact = String::new();
        render(&doc, &mut None, &mut compact);
        let mut spaced = String::from(rng.pick(&["", " ", "\n"]));
        render(&doc, &mut Some(&mut rng), &mut spaced);
        spaced.push_str(rng.pick(&["", "\n", " \r\n"]));

        let out = minify_json(&spaced);
        if spaced == compact {
            assert_eq!(out, None, "seed {seed}");
        } else {
            assert_eq!(out.as_deref(), Some(compact.as_str()), "seed {seed}");
        }
        // Validates the generator only: both sides are generator output.
        let a: serde_json::Value = serde_json::from_str(&spaced).unwrap();
        let b: serde_json::Value = serde_json::from_str(&compact).unwrap();
        assert_eq!(a, b, "seed {seed}");
        assert_eq!(minify_json(&compact), None, "seed {seed}: not idempotent");

        // A strict prefix of the trimmed text lacks the closing bracket.
        let text = spaced.trim();
        for _ in 0..4 {
            let mut cut = rng.below(text.len());
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            let prefix = &text[..cut];
            let result = compress(prefix, &ctx);
            assert!(
                matches!(&result.output, Cow::Borrowed(s) if std::ptr::eq(*s, prefix)),
                "seed {seed}: {prefix:?}"
            );
            assert_eq!(result.report.outcome, Outcome::Skipped, "seed {seed}");
            assert_eq!(
                result.report.reason,
                Some(Reason::UnsupportedFormat),
                "seed {seed}"
            );
        }
    }
}
