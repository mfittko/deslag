use std::{
    fs,
    path::{Path, PathBuf},
};

use deslag_core::{
    CompressionContext, EstimateCounter, FIRST_SLICE_JSON_MINIFY_CONTEXT, TokenCounter,
    TokenMethod, compress, minify_json,
};

/// Recorded bounds, see `tests/fixtures/tokens/README.md`. Relative error is
/// `(estimate - reference) / reference`.
/// Every fixture except `cjk.txt`. `nested.min.json` sets it: the savings
/// ceiling on cl100k_base forces an estimate 27% above o200k_base there.
const ASCII_FIXTURE_ERROR_BOUND: f64 = 0.28;
/// Applies only to `cjk.txt`. The two references differ by 44% on it.
const CJK_FIXTURE_ERROR_BOUND: f64 = 0.40;
const AGGREGATE_ERROR_BOUND: f64 = 0.10;
/// The estimated savings ratio may undershoot the reference by this much. It
/// may never exceed the reference. `nested_tab.json` sets it: one estimate
/// cannot sit below cl100k_base and within 0.10 of o200k_base there.
const SAVINGS_UNDERSHOOT: f64 = 0.17;

const PAIRS: [(&str, &str); 4] = [
    ("pretty.json", "pretty.min.json"),
    ("records.json", "records.min.json"),
    ("nested.json", "nested.min.json"),
    ("nested_tab.json", "nested_tab.min.json"),
];

/// Pinned estimate per fixture. A heuristic change that moves any value must
/// bump `EstimateCounter::TOKENIZER` and update the README tables.
const GOLDEN: [(&str, u64); 12] = [
    ("pretty.json", 217),
    ("pretty.min.json", 158),
    ("records.json", 429),
    ("records.min.json", 288),
    ("nested.json", 424),
    ("nested.min.json", 270),
    ("nested_tab.json", 424),
    ("nested_tab.min.json", 270),
    ("log.txt", 396),
    ("source_code.txt", 197),
    ("prose.txt", 120),
    ("cjk.txt", 149),
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
fn estimates_match_golden_counts() {
    let (_, rows) = reference();
    let names: Vec<&str> = rows.iter().map(|(n, _)| n.as_str()).collect();
    let golden: Vec<&str> = GOLDEN.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, golden, "GOLDEN must list every reference.tsv row");
    for (name, want) in GOLDEN {
        assert_eq!(EstimateCounter.count(&read(name)).count, want, "{name}");
    }
}

#[test]
fn non_bmp_characters_cost_two_tokens() {
    // References: five U+1F600 is 10 tokens in cl100k_base. The ZWJ family
    // U+1F468 U+200D U+1F469 U+200D U+1F467 is 8 in o200k_base and 13 in
    // cl100k_base.
    assert_eq!(EstimateCounter.count("😀😀😀😀😀").count, 10);
    assert_eq!(EstimateCounter.count("👨\u{200d}👩\u{200d}👧").count, 8);
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
    assert!(!json.contains(r#""method":"exact""#));
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
            let bound = if name == "cjk.txt" {
                CJK_FIXTURE_ERROR_BOUND
            } else {
                ASCII_FIXTURE_ERROR_BOUND
            };
            eprintln!("{enc}\t{name}\test={est}\tref={refc}\terr={err:+.3}");
            assert!(err.abs() <= bound, "{enc} {name}: {err:+.3}");
            est_sum += est;
            ref_sum += refc;
        }
        let err = (est_sum - ref_sum) / ref_sum;
        eprintln!("{enc}\taggregate\test={est_sum}\tref={ref_sum}\terr={err:+.3}");
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
            assert!(delta <= 0.0, "{enc} {pretty}: {delta:+.3}");
            assert!(-delta <= SAVINGS_UNDERSHOOT, "{enc} {pretty}: {delta:+.3}");
        }
    }
}

/// Collects every `.rs` file under `dir`, recursively.
fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Returns the first manifest line that adds a non-dev dependency: a non-dev
/// `[..dependencies]` header, or a `dependencies`, `build-dependencies` or
/// `target.` key outside the `[dev-dependencies]` table.
fn manifest_violation(manifest: &str) -> Option<&str> {
    let mut in_dev = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_dev = line == "[dev-dependencies]";
            if !in_dev && line.contains("dependencies") {
                return Some(line);
            }
        } else if !in_dev
            && ["dependencies", "build-dependencies", "target."]
                .iter()
                .any(|key| line.starts_with(key))
        {
            return Some(line);
        }
    }
    None
}

/// Returns the first process-spawn or network needle in `src`, including
/// grouped `use std::{net, ..}` and `use std::{process, ..}` imports.
fn source_violation(src: &str) -> Option<&'static str> {
    const NEEDLES: [&str; 9] = [
        "std::process",
        "process::",
        "Command::new",
        "std::net",
        "net::",
        "TcpStream",
        "TcpListener",
        "UdpSocket",
        "ToSocketAddrs",
    ];
    if let Some(needle) = NEEDLES.into_iter().find(|n| src.contains(n)) {
        return Some(needle);
    }
    for (start, _) in src.match_indices("std::{") {
        let group = src[start..].split(';').next().unwrap_or_default();
        for module in ["net", "process"] {
            if group
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|token| token == module)
            {
                return Some(module);
            }
        }
    }
    None
}

#[test]
fn core_has_no_dependencies_or_network_client() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert_eq!(manifest_violation(&manifest), None, "Cargo.toml");
    let mut files = Vec::new();
    rust_sources(&root.join("src"), &mut files);
    let build = root.join("build.rs");
    if build.exists() {
        files.push(build);
    }
    assert!(!files.is_empty());
    for path in files {
        let src = fs::read_to_string(&path).unwrap();
        assert_eq!(source_violation(&src), None, "{}", path.display());
    }
}

#[test]
fn no_dependency_guard_rejects_every_form() {
    for ok in [
        "[package]\nname = \"x\"\n\n[dev-dependencies]\nserde_json = \"1\"\ntarget-lexicon = \"1\"\n",
        "[package]\nname = \"x\"\n",
    ] {
        assert_eq!(manifest_violation(ok), None, "{ok}");
    }
    for bad in [
        "[dependencies]\nreqwest = \"0.12\"",
        "[build-dependencies]\ncc = \"1\"",
        "[target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"",
        "dependencies.reqwest = \"0.12\"\n[package]",
        "dependencies = { reqwest = \"0.12\" }\n[package]",
        "build-dependencies.cc = \"1\"\n[package]",
        "[dev-dependencies]\nx = \"1\"\n[dependencies]\ny = \"1\"",
    ] {
        assert!(manifest_violation(bad).is_some(), "{bad}");
    }
}

#[test]
fn no_network_guard_rejects_every_form() {
    assert_eq!(
        source_violation("use std::{borrow::Cow, fmt::Write, time::Duration};"),
        None
    );
    for bad in [
        "use std::process::{Command, Stdio};",
        "Command::new(\"curl\")",
        "Command::new(\"curl\").spawn()",
        "use std::{net, io}; fn f() { net::TcpListener::bind(\"x\"); }",
        "use std::{io, net};",
        "use std::{io, process};",
        "use std::process::Command;",
        "std::net::TcpStream::connect(\"x\")",
        "fn f<A: ToSocketAddrs>(a: A) {}",
    ] {
        assert!(source_violation(bad).is_some(), "{bad}");
    }
}
