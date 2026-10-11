use std::{fs, path::PathBuf};

use deslag_core::{
    CompressionContext, EstimateCounter, FIRST_SLICE_JSON_MINIFY_CONTEXT, TokenCounter,
    TokenMethod, compress, minify_json,
};

/// Recorded bounds, see `tests/fixtures/tokens/README.md`. Relative error is
/// `(estimate - reference) / reference`.
const FIXTURE_ERROR_BOUND: f64 = 0.40;
const AGGREGATE_ERROR_BOUND: f64 = 0.10;
/// The estimated savings ratio may undershoot the reference by this much and
/// may never exceed it by more than `SAVINGS_OVERSHOOT`.
const SAVINGS_UNDERSHOOT: f64 = 0.10;
const SAVINGS_OVERSHOOT: f64 = 0.0;

const PAIRS: [(&str, &str); 2] = [
    ("pretty.json", "pretty.min.json"),
    ("records.json", "records.min.json"),
];

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tokens")
}

fn read(name: &str) -> String {
    fs::read_to_string(dir().join(name)).unwrap()
}

fn estimate(name: &str) -> f64 {
    EstimateCounter.count(&read(name)).count as f64
}

/// Rows of `reference.tsv`: file name, then one count per encoding.
fn reference() -> (Vec<String>, Vec<(String, Vec<f64>)>) {
    let text = read("reference.tsv");
    let mut lines = text.lines().map(|l| l.split('\t'));
    let encodings = lines.next().unwrap().skip(1).map(String::from).collect();
    let rows = lines
        .map(|mut cols| {
            let name = cols.next().unwrap().to_string();
            (name, cols.map(|c| c.parse().unwrap()).collect())
        })
        .collect();
    (encodings, rows)
}

fn ref_count(rows: &[(String, Vec<f64>)], name: &str, enc: usize) -> f64 {
    rows.iter().find(|(n, _)| n == name).unwrap().1[enc]
}

#[test]
fn every_estimate_is_labeled() {
    for text in ["", " ", "x", "{\"a\": 1}", "東京"] {
        let c = EstimateCounter.count(text);
        assert_eq!(c.method, TokenMethod::Estimate);
        assert_eq!(c.tokenizer, "deslag-estimate-v1");
    }
    assert_eq!(EstimateCounter.count("").count, 0);
}

#[test]
fn report_carries_estimate_labels() {
    let ctx = CompressionContext {
        token_counter: Some(&EstimateCounter),
        ..FIRST_SLICE_JSON_MINIFY_CONTEXT
    };
    let report = compress(&read("pretty.json"), &ctx).report;
    for t in [
        report.original_tokens,
        report.candidate_tokens,
        report.emitted_tokens,
    ] {
        let t = t.unwrap();
        assert_eq!(t.method, TokenMethod::Estimate);
        assert_eq!(t.tokenizer, EstimateCounter::TOKENIZER);
    }
    let json = report.to_json();
    assert!(json.contains(r#""method":"estimate","tokenizer":"deslag-estimate-v1""#));
    assert!(!json.contains("exact"));
}

#[test]
fn missing_tokenizer_stays_unknown() {
    let report = compress(&read("pretty.json"), &FIRST_SLICE_JSON_MINIFY_CONTEXT).report;
    let json = report.to_json();
    for key in ["original_tokens", "candidate_tokens", "emitted_tokens"] {
        assert!(json.contains(&format!("\"{key}\":null")), "{key} in {json}");
    }
}

#[test]
fn minified_fixtures_match_the_core_minifier() {
    for (pretty, min) in PAIRS {
        assert_eq!(minify_json(&read(pretty)).unwrap(), read(min), "{pretty}");
    }
}

#[test]
fn estimates_stay_within_recorded_bounds() {
    let (encodings, rows) = reference();
    for (enc_i, enc) in encodings.iter().enumerate() {
        let (mut est_sum, mut ref_sum) = (0.0, 0.0);
        for (name, counts) in &rows {
            let (est, refc) = (estimate(name), counts[enc_i]);
            let err = (est - refc) / refc;
            eprintln!("{enc}\t{name}\test={est}\tref={refc}\terr={err:+.3}");
            assert!(err.abs() <= FIXTURE_ERROR_BOUND, "{enc} {name}: {err:+.3}");
            est_sum += est;
            ref_sum += refc;
        }
        let err = (est_sum - ref_sum) / ref_sum;
        eprintln!("{enc}\taggregate\terr={err:+.3}");
        assert!(
            err.abs() <= AGGREGATE_ERROR_BOUND,
            "{enc} aggregate: {err:+.3}"
        );
    }
}

#[test]
fn minify_savings_are_not_overestimated() {
    let (encodings, rows) = reference();
    for (enc_i, enc) in encodings.iter().enumerate() {
        for (pretty, min) in PAIRS {
            let ratio = |p: f64, m: f64| (p - m) / p;
            let est = ratio(estimate(pretty), estimate(min));
            let refr = ratio(
                ref_count(&rows, pretty, enc_i),
                ref_count(&rows, min, enc_i),
            );
            let delta = est - refr;
            eprintln!(
                "{enc}\t{pretty}\test_savings={est:.3}\tref_savings={refr:.3}\tdelta={delta:+.3}"
            );
            assert!(delta <= SAVINGS_OVERSHOOT, "{enc} {pretty}: {delta:+.3}");
            assert!(-delta <= SAVINGS_UNDERSHOOT, "{enc} {pretty}: {delta:+.3}");
        }
    }
}

#[test]
fn core_has_no_dependencies_or_network_client() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(!manifest.contains("[dependencies]"));
    for entry in fs::read_dir(root.join("src")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        for needle in ["std::net", "TcpStream", "UdpSocket"] {
            assert!(!src.contains(needle), "{needle} in {}", path.display());
        }
    }
}
