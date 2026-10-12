//! Offline checks of the evaluation harness over the committed corpus. Only the
//! mock backend runs here; no test reaches a paid model.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_deslag-eval");

fn manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/evaluations/manifest.json")
}

fn eval(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_remove("DESLAG_EVAL_LIVE")
        .output()
        .unwrap()
}

fn mock_run(seed: &str, name: &str) -> (String, Vec<Value>) {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let m = manifest();
    let o = eval(&[
        "run",
        "--manifest",
        m.to_str().unwrap(),
        "--backend",
        "mock",
        "--seed",
        seed,
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let text = std::fs::read_to_string(&out).unwrap();
    let lines = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    (text, lines)
}

#[test]
fn manifest_covers_every_category_and_control() {
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(manifest()).unwrap()).unwrap();
    let tasks = doc["tasks"].as_array().unwrap();
    let dir = manifest().parent().unwrap().to_path_buf();
    let mut categories = BTreeSet::new();
    for t in tasks {
        assert!(dir.join(t["input"].as_str().unwrap()).is_file(), "{t}");
        let answers = t["answers"].as_array().unwrap();
        assert!(
            !answers.is_empty()
                && answers
                    .iter()
                    .all(|a| !a.as_str().unwrap().trim().is_empty())
        );
        assert!(!t["arms"].as_array().unwrap().is_empty());
        categories.insert(t["category"].as_str().unwrap());
    }
    for c in [
        "api_query",
        "code_search",
        "failure_diagnosis",
        "issue_triage",
        "logs",
        "aggregation",
        "exact_id",
        "buried_evidence",
        "incompressible",
        "evidence_drop",
    ] {
        let n = tasks.iter().filter(|t| t["category"] == c).count();
        assert!(n >= 2, "category {c} has {n} tasks");
    }
    // At least one buried-evidence input has 100 or more rows.
    let largest = tasks
        .iter()
        .filter(|t| t["category"] == "buried_evidence")
        .map(|t| std::fs::read_to_string(dir.join(t["input"].as_str().unwrap())).unwrap())
        .filter_map(|s| {
            serde_json::from_str::<Value>(&s)
                .ok()?
                .as_array()
                .map(Vec::len)
        })
        .max();
    assert!(largest >= Some(100), "{largest:?}");
}

#[test]
fn mock_run_detects_evidence_loss_and_keeps_controls_unchanged() {
    let (_, lines) = mock_run("16", "mock-16.jsonl");
    let tasks =
        serde_json::from_str::<Value>(&std::fs::read_to_string(manifest()).unwrap()).unwrap();
    let expected: usize = tasks["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| 1 + t["arms"].as_array().unwrap().len())
        .sum();
    assert_eq!(lines.len(), expected);
    let orders: BTreeSet<u64> = lines.iter().map(|l| l["order"].as_u64().unwrap()).collect();
    assert_eq!(orders.len(), expected);
    for l in &lines {
        match l["control"].as_str() {
            // Every transform is lossless, so the mock finds every answer.
            None => assert_eq!(l["success"], true, "{l}"),
            Some("incompressible") => {
                assert_eq!(l["variant_unchanged"], true, "{l}");
                assert_eq!(l["success"], true, "{l}");
            }
            Some("evidence_drop") if l["arm"] == "drop-evidence" => {
                assert_eq!(l["success"], false, "{l}");
                assert_eq!(l["error_kind"], "abstain", "{l}");
            }
            Some("evidence_drop") => assert_eq!(l["success"], true, "{l}"),
            Some(other) => panic!("unknown control {other}"),
        }
    }
    // Every transform arm changes at least one input, so the corpus exercises it.
    for arm in [
        "json-minify",
        "table-csv",
        "table-mdkv",
        "lines-v1",
        "drop-evidence",
    ] {
        assert!(
            lines
                .iter()
                .any(|l| l["arm"] == arm && l["variant_unchanged"] == false),
            "{arm}"
        );
    }
}

#[test]
fn run_order_is_seeded() {
    let (a, _) = mock_run("7", "mock-7a.jsonl");
    let (b, _) = mock_run("7", "mock-7b.jsonl");
    let (c, _) = mock_run("8", "mock-8.jsonl");
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn report_renders_format_comparison_and_controls() {
    mock_run("16", "mock-report.jsonl");
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("mock-report.jsonl");
    let o = eval(&["report", path.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let md = String::from_utf8(o.stdout).unwrap();
    assert!(
        md.contains("| table-v1 CSV | table-csv | 10 | 10/10 (100.0%) |"),
        "{md}"
    );
    assert!(
        md.contains("| Markdown-KV | table-mdkv | 10 | 10/10 (100.0%) |"),
        "{md}"
    );
    assert!(md.contains("| evidence_drop | drop-evidence | 2 | 2/2 (100.0%) | 0/2 (0.0%) | 0 | 2 | evidence loss detected |"), "{md}");
    assert!(!md.contains("a transform changed a control input"), "{md}");
    // The pilot is below 81 pairs per arm, so no arm can pass.
    assert!(
        md.contains("| table-csv | 10 | 0 |")
            && md.contains("not shown")
            && !md.contains("passes |"),
        "{md}"
    );
}

#[test]
fn claude_backend_refuses_without_live_opt_in() {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("refused.jsonl");
    let m = manifest();
    let o = Command::new(BIN)
        .args([
            "run",
            "--manifest",
            m.to_str().unwrap(),
            "--backend",
            "claude",
            "--model",
            "x",
        ])
        .args([
            "--budget-usd",
            "1",
            "--seed",
            "1",
            "--out",
            out.to_str().unwrap(),
        ])
        .env_remove("DESLAG_EVAL_LIVE")
        // Even a bug past the check could not start a client.
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("DESLAG_EVAL_LIVE=1"));
    assert!(!out.exists());
}

#[test]
fn claude_backend_needs_model_and_budget() {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("unbudgeted.jsonl");
    let m = manifest();
    let o = Command::new(BIN)
        .args([
            "run",
            "--manifest",
            m.to_str().unwrap(),
            "--backend",
            "claude",
        ])
        .args(["--seed", "1", "--out", out.to_str().unwrap()])
        .env("DESLAG_EVAL_LIVE", "1")
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(!out.exists());
}

#[test]
fn estimate_prints_a_cost() {
    let m = manifest();
    let o = eval(&["estimate", "--manifest", m.to_str().unwrap()]);
    assert!(o.status.success());
    let md = String::from_utf8(o.stdout).unwrap();
    assert!(
        md.contains("| Estimated cost | USD ") && md.contains("| Hard cap (66 runs"),
        "{md}"
    );
}

#[test]
fn report_pairs_within_each_seed() {
    let (a, _) = mock_run("16", "seed-a.jsonl");
    let (_, lines) = mock_run("17", "seed-b.jsonl");
    // Seed 17's baselines all fail, so every seed-17 pair is discordant for the arm.
    let mut text = a;
    for mut l in lines {
        if l["arm"] == "baseline" {
            l["success"] = false.into();
            l["error_kind"] = "wrong".into();
        }
        text += &format!("{l}\n");
    }
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("two-seeds.jsonl");
    std::fs::write(&path, text).unwrap();
    let o = eval(&["report", path.to_str().unwrap()]);
    let md = String::from_utf8(o.stdout).unwrap();
    assert!(md.contains("| seed | 16, 17 |"), "{md}");
    assert!(
        md.contains("| runs without a baseline partner | 0 |"),
        "{md}"
    );
    assert!(md.contains("| json-minify | 22 | 0 | 0 | 11 |"), "{md}");
    assert!(md.contains("| table-v1 CSV | table-csv | 20 |"), "{md}");
}
