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
    // `run` refuses an existing --out path.
    let _ = std::fs::remove_file(&out);
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
    // lines-v2 runs on the incompressible control, which the loop above keeps unchanged.
    assert!(
        lines
            .iter()
            .any(|l| l["arm"] == "lines-v2" && l["control"] == "incompressible")
    );
    // Every transform arm changes at least one input, so the corpus exercises it.
    for arm in [
        "json-minify",
        "table-csv",
        "table-mdkv",
        "lines-v1",
        "lines-v2",
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
    // The mock records wall_ms as 0, so identical runs are byte-identical.
    let order = |seed, name| -> Vec<Value> {
        let (_, lines) = mock_run(seed, name);
        for l in &lines {
            assert_eq!(l["wall_ms"], 0, "{l}");
        }
        lines
    };
    let (a, b, c) = (
        order("7", "mock-7a.jsonl"),
        order("7", "mock-7b.jsonl"),
        order("8", "mock-8.jsonl"),
    );
    assert_eq!(a, b);
    // Every line carries its seed, so compare only the executed (task, arm) sequence.
    let sequence = |lines: &[Value]| {
        let mut l: Vec<&Value> = lines.iter().collect();
        l.sort_by_key(|l| l["order"].as_u64());
        l.iter()
            .map(|l| (l["task"].clone(), l["arm"].clone()))
            .collect::<Vec<_>>()
    };
    assert_ne!(sequence(&a), sequence(&c));
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
    assert!(
        md.contains(
            "| evidence_drop | lines-v2 | 1 | 1/1 (100.0%) | 1/1 (100.0%) | 0 | 0 | evidence kept |"
        ),
        "{md}"
    );
    assert!(md.contains("| incompressible | lines-v2 | 2 | 2/2 (100.0%) | 2/2 (100.0%) | 2 | 0 | all variants unchanged |"), "{md}");
    assert!(!md.contains("a transform changed a control input"), "{md}");
    // Edit one arm's control results and check one control row.
    let (_, lines) = mock_run("16", "mock-report-fail.jsonl");
    let (v2, drop, inc) = (
        "| evidence_drop | lines-v2 | 1 |",
        "| evidence_drop | drop-evidence | 2 |",
        "| incompressible | lines-v2 | 2 |",
    );
    for (control, edit_arm, success, row, result) in [
        ("evidence_drop", "baseline", false, v2, "baseline failed"),
        ("evidence_drop", "baseline", false, drop, "baseline failed"),
        (
            "evidence_drop",
            "lines-v2",
            false,
            v2,
            "a transform lost evidence",
        ),
        (
            "evidence_drop",
            "drop-evidence",
            true,
            drop,
            "evidence loss not detected",
        ),
        (
            "incompressible",
            "lines-v2",
            true,
            inc,
            "a transform changed a control input",
        ),
    ] {
        let mut text = String::new();
        let mut edited = false;
        for mut l in lines.clone() {
            if l["control"] == control && l["arm"] == edit_arm {
                if control == "incompressible" {
                    // One changed line out of two must fail the control.
                    if !edited {
                        l["variant_unchanged"] = false.into();
                    }
                } else {
                    l["success"] = success.into();
                    l["error_kind"] = if success { "none" } else { "wrong" }.into();
                }
                edited = true;
            }
            text += &format!("{l}\n");
        }
        assert!(edited, "no {control} line for {edit_arm}");
        let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("mock-report-fail-edit.jsonl");
        std::fs::write(&path, text).unwrap();
        let o = eval(&["report", path.to_str().unwrap()]);
        let md = String::from_utf8(o.stdout).unwrap();
        let line = md.lines().find(|l| l.starts_with(row)).unwrap_or("");
        assert!(line.ends_with(&format!("| {result} |")), "{md}");
    }
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
fn claude_backend_needs_model_and_valid_caps() {
    const NEEDS: &str = "needs --model, --budget-usd >= 0 and --per-run-usd > 0";
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("unbudgeted.jsonl");
    let m = manifest();
    for (extra, msg) in [
        (&["--budget-usd", "1"][..], NEEDS),
        (&["--budget-usd", "1", "--model", ""], NEEDS),
        (&["--budget-usd", "1", "--model", "   "], NEEDS),
        (&["--model", "x"], "--budget-usd is required"),
        (&["--model", "x", "--budget-usd", "-1"], NEEDS),
        (&["--model", "x", "--budget-usd", "NaN"], NEEDS),
        (&["--model", "x", "--budget-usd", "inf"], NEEDS),
        (
            &["--model", "x", "--budget-usd", "1", "--per-run-usd", "0"],
            NEEDS,
        ),
        (
            &["--model", "x", "--budget-usd", "1", "--per-run-usd", "-1"],
            NEEDS,
        ),
        (
            &["--model", "x", "--budget-usd", "1", "--per-run-usd", "NaN"],
            NEEDS,
        ),
        // Accepted: the guard passes and the empty PATH stops the client start.
        (
            &["--model", "claude-haiku-4-5", "--budget-usd", "1"],
            "cannot start claude",
        ),
    ] {
        let o = Command::new(BIN)
            .args([
                "run",
                "--manifest",
                m.to_str().unwrap(),
                "--backend",
                "claude",
            ])
            .args(["--seed", "1", "--out", out.to_str().unwrap()])
            .args(extra)
            .env("DESLAG_EVAL_LIVE", "1")
            .env("PATH", "")
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(2), "{extra:?}");
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(err.contains(msg), "{extra:?}: {err}");
        assert!(!out.exists(), "{extra:?}");
    }
}

#[test]
fn estimate_validates_prices_and_cap() {
    let m = manifest();
    for (extra, ok) in [
        (&["--per-run-usd", "0.30"][..], true),
        (&["--input-usd-per-mtok", "0"], true),
        (&["--per-run-usd", "-1"], false),
        (&["--per-run-usd", "0"], false),
        (&["--input-usd-per-mtok", "NaN"], false),
        (&["--output-usd-per-mtok", "inf"], false),
    ] {
        let o = Command::new(BIN)
            .args(["estimate", "--manifest", m.to_str().unwrap()])
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(o.status.success(), ok, "{extra:?}");
        if !ok {
            assert_eq!(o.status.code(), Some(2), "{extra:?}");
            assert!(o.stdout.is_empty(), "{extra:?}");
        }
    }
}

#[test]
fn estimate_refuses_token_overflow() {
    let m = manifest();
    for (extra, ok) in [
        (&["--output-tokens", "256"][..], true),
        (&["--overhead-tokens", "0"], true),
        (&["--output-tokens", "18446744073709551615"], false),
        (&["--overhead-tokens", "18446744073709551615"], false),
    ] {
        let o = Command::new(BIN)
            .args(["estimate", "--manifest", m.to_str().unwrap()])
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(o.status.success(), ok, "{extra:?}");
        if !ok {
            assert_eq!(o.status.code(), Some(2), "{extra:?}");
            assert!(o.stdout.is_empty(), "{extra:?}");
            let err = String::from_utf8_lossy(&o.stderr);
            assert!(err.contains("estimate needs"), "{extra:?}: {err}");
        }
    }
}

#[test]
fn report_gives_an_arm_without_non_control_pairs_no_data() {
    // lines-v2 then runs only on control tasks.
    let (_, lines) = mock_run("16", "control-only.jsonl");
    let text: String = lines
        .iter()
        .filter(|l| l["arm"] != "lines-v2" || !l["control"].is_null())
        .map(|l| format!("{l}\n"))
        .collect();
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("control-only-edit.jsonl");
    std::fs::write(&path, text).unwrap();
    let o = eval(&["report", path.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let md = String::from_utf8(o.stdout).unwrap();
    let table = md
        .split("### Non-inferiority per arm")
        .nth(1)
        .and_then(|s| s.split("### ").next())
        .unwrap();
    assert!(
        table.contains("| lines-v2 | 0 | 0 | 0 | 0 | n/a | n/a | n/a | n/a | no data |"),
        "{table}"
    );
    assert!(table.contains("| table-csv | 10 | 0 |"), "{table}");
    assert!(table.contains("not shown"), "{table}");
    assert!(!table.contains("| drop-evidence |"), "{table}");
    assert!(!table.contains("| baseline |"), "{table}");
}

#[test]
fn estimate_prints_a_cost() {
    let m = manifest();
    let o = eval(&["estimate", "--manifest", m.to_str().unwrap()]);
    assert!(o.status.success());
    let md = String::from_utf8(o.stdout).unwrap();
    assert!(
        md.contains("| Estimated cost | USD ")
            && md.contains("| Cap if every call stays within --per-run-usd (73 runs"),
        "{md}"
    );
}

#[test]
fn report_takes_one_run_per_file() {
    let (a, lines) = mock_run("16", "seed-a.jsonl");
    let (b, _) = mock_run("17", "seed-b.jsonl");
    let first = a.lines().next().unwrap();
    let mut other_model = lines[1].clone();
    other_model["model"] = "sonnet".into();
    // Repeated runs of a task are not independent pairs, so each case is refused.
    for (name, text, msg) in [
        ("two-seeds", format!("{a}{b}"), "differs from line 1"),
        (
            "two-models",
            format!("{first}\n{other_model}\n"),
            "differs from line 1",
        ),
        ("duplicate", format!("{a}{first}\n"), "duplicate task"),
    ] {
        let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.jsonl"));
        std::fs::write(&path, text).unwrap();
        let o = eval(&["report", path.to_str().unwrap()]);
        assert_eq!(o.status.code(), Some(2), "{name}");
        assert!(String::from_utf8_lossy(&o.stderr).contains(msg), "{name}");
        assert!(o.stdout.is_empty(), "{name}");
    }
}

#[test]
fn report_escapes_table_cells() {
    let (_, lines) = mock_run("16", "escape.jsonl");
    let mut text = String::new();
    for mut l in lines {
        l["client_version"] = "2.1 | passes\n| forged | row |".into();
        text += &format!("{l}\n");
    }
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("escape-edit.jsonl");
    std::fs::write(&path, text).unwrap();
    let o = eval(&["report", path.to_str().unwrap()]);
    let md = String::from_utf8(o.stdout).unwrap();
    assert!(
        md.contains("| client version | 2.1 \\| passes \\| forged \\| row \\| |"),
        "{md}"
    );
    assert!(!md.lines().any(|l| l.starts_with("| forged")), "{md}");
}

/// A fresh, empty directory under the target tmpdir.
fn fresh_dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn flag_values_cannot_be_flags() {
    let o = eval(&["run", "--manifest", "--backend", "mock"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("--manifest needs a value"));
}

#[test]
fn repeated_flags_are_rejected() {
    let o = eval(&["run", "--seed", "1", "--seed", "2"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("--seed given twice"));
}

#[test]
fn manifest_rejects_bad_tolerance_and_uncontrolled_drop_evidence() {
    let dir = fresh_dir("bad-manifests");
    let input = manifest()
        .parent()
        .unwrap()
        .join("inputs/api_query_repo.json");
    let task = |extra: &str| {
        let answers = if extra.contains("answers") {
            ""
        } else {
            r#""answers": ["1873"], "#
        };
        format!(
            r#"{{"tasks": [{{"id": "t", "category": "api_query", "input": {:?}, "question": "q", {answers}"rule": "number", {extra}}}]}}"#,
            input.to_str().unwrap()
        )
    };
    for (name, extra, msg) in [
        (
            "control-typo",
            r#""arms": ["json-minify"], "control": "incompressable""#,
            "unknown control",
        ),
        (
            "control-dash",
            r#""arms": ["json-minify"], "control": "evidence-drop""#,
            "unknown control",
        ),
        (
            "control-space",
            r#""arms": ["json-minify"], "control": "incompressible ""#,
            "unknown control",
        ),
        (
            "control-number",
            r#""arms": ["json-minify"], "control": 1"#,
            "control must be a string",
        ),
        (
            "control-bool",
            r#""arms": ["json-minify"], "control": true"#,
            "control must be a string",
        ),
        (
            "control-array",
            r#""arms": ["json-minify"], "control": ["incompressible"]"#,
            "control must be a string",
        ),
        (
            "control-object",
            r#""arms": ["json-minify"], "control": {}"#,
            "control must be a string",
        ),
        (
            "control-other",
            r#""arms": ["json-minify"], "control": "other""#,
            "unknown control",
        ),
        (
            "number-comma",
            r#""arms": ["json-minify"], "answers": ["1,873"]"#,
            "finite numbers",
        ),
        (
            "number-nan",
            r#""arms": ["json-minify"], "answers": ["NaN"]"#,
            "finite numbers",
        ),
        (
            "neg",
            r#""arms": ["json-minify"], "tolerance": -1"#,
            "tolerance",
        ),
        (
            "str",
            r#""arms": ["json-minify"], "tolerance": "1""#,
            "tolerance",
        ),
        (
            "drop",
            r#""arms": ["drop-evidence"], "evidence": "trunk""#,
            "control evidence_drop",
        ),
        (
            "dup-arm",
            r#""arms": ["json-minify", "json-minify"]"#,
            "duplicate arm json-minify",
        ),
        (
            "dup-arm-later",
            r#""arms": ["table-csv", "lines-v1", "table-csv"]"#,
            "duplicate arm table-csv",
        ),
    ] {
        let path = dir.join(format!("{name}.json"));
        std::fs::write(&path, task(extra)).unwrap();
        let o = eval(&["variants", path.to_str().unwrap()]);
        assert_eq!(o.status.code(), Some(2), "{name}");
        assert!(String::from_utf8_lossy(&o.stderr).contains(msg), "{name}");
    }
    let empty = dir.join("empty.json");
    std::fs::write(&empty, r#"{"tasks": []}"#).unwrap();
    let o = eval(&["variants", empty.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("empty tasks array"));
    for (name, extra) in [
        ("ok", r#""arms": ["json-minify"], "tolerance": 0.5"#),
        ("no-control", r#""arms": ["json-minify"]"#),
        (
            "null-control",
            r#""arms": ["json-minify"], "control": null"#,
        ),
        (
            "evidence-drop-control",
            r#""arms": ["json-minify"], "control": "evidence_drop""#,
        ),
        ("two-arms", r#""arms": ["json-minify", "table-csv"]"#),
        ("no-arms", r#""arms": []"#),
        (
            "incompressible",
            r#""arms": ["json-minify"], "control": "incompressible""#,
        ),
        (
            "evidence-drop",
            r#""arms": ["drop-evidence"], "control": "evidence_drop", "evidence": "trunk""#,
        ),
        ("decimal", r#""arms": ["json-minify"], "answers": ["18.5"]"#),
    ] {
        let ok = dir.join(format!("{name}.json"));
        std::fs::write(&ok, task(extra)).unwrap();
        let o = eval(&["variants", ok.to_str().unwrap()]);
        assert!(
            o.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }
}

#[test]
fn run_refuses_an_existing_out_path() {
    let (first, _) = mock_run("16", "mock-twice.jsonl");
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("mock-twice.jsonl");
    let m = manifest();
    let o = eval(&[
        "run",
        "--manifest",
        m.to_str().unwrap(),
        "--backend",
        "mock",
        "--seed",
        "17",
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("exists"));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), first);
}

/// Live-backend runs against a fake `claude` script. `mode` selects its reply:
/// `ok` (no cost field), `is_error`, `bad` (malformed JSON, exit 1), `pricey`
/// or `cost-<json number>`.
#[cfg(unix)]
fn fake_live_run(mode: &str, out_exists: bool) -> (Output, Vec<Value>, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let root = fresh_dir(&format!("fake-claude-{mode}-{out_exists}"));
    let bin = root.join("bin");
    let tmp = root.join("tmp");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&tmp).unwrap();
    let script = bin.join("claude");
    std::fs::write(
        &script,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo "9.9.9 (fake)"; exit 0; fi
# Fail unless the cost cap and isolation args are present.
budget=; tools=; mcp=; prev=
for a in "$@"; do
  [ "$prev" = --max-budget-usd ] && [ "$a" = 0.3 ] && budget=1
  [ "$prev" = --tools ] && [ -z "$a" ] && tools=1
  [ "$a" = --strict-mcp-config ] && mcp=1
  prev=$a
done
[ "$budget$tools$mcp" = 111 ] || exit 1
cat >/dev/null
case "$FAKE_CLAUDE_MODE" in
  ok) echo '{"type":"result","is_error":false,"result":"trunk-2026","usage":{"input_tokens":3,"output_tokens":2},"num_turns":1}' ;;
  is_error) echo '{"type":"result","is_error":true,"result":"boom","num_turns":1}' ;;
  pricey) echo '{"type":"result","is_error":false,"result":"x","total_cost_usd":0.5,"num_turns":1}' ;;
  cost-*) echo "{\"type\":\"result\",\"is_error\":false,\"result\":\"x\",\"total_cost_usd\":${FAKE_CLAUDE_MODE#cost-},\"num_turns\":1}" ;;
  *) echo 'not json'; exit 1 ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = root.join("live.jsonl");
    if out_exists {
        std::fs::write(&out, "").unwrap();
    }
    let m = manifest();
    let o = Command::new(BIN)
        .args([
            "run",
            "--manifest",
            m.to_str().unwrap(),
            "--backend",
            "claude",
        ])
        .args([
            "--model",
            "fake",
            "--seed",
            "16",
            "--out",
            out.to_str().unwrap(),
        ])
        .args(["--budget-usd", "1.0", "--per-run-usd", "0.3"])
        .env("PATH", format!("{}:/bin:/usr/bin", bin.display()))
        .env("TMPDIR", &tmp)
        .env("DESLAG_EVAL_LIVE", "1")
        .env("FAKE_CLAUDE_MODE", mode)
        .output()
        .unwrap();
    let leaked: Vec<_> = std::fs::read_dir(&tmp)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("deslag-eval-"))
        .collect();
    assert!(leaked.is_empty(), "{leaked:?}");
    let lines = std::fs::read_to_string(&out)
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    (o, lines, out)
}

#[cfg(unix)]
#[test]
fn live_backend_stops_at_the_budget_and_cleans_up() {
    // 1.0 USD at 0.3 per uncosted call: floor(1.0 / 0.3) = 3 calls.
    for (mode, kinds) in [
        ("ok", &["none", "wrong", "abstain"][..]),
        ("is_error", &["backend"][..]),
        ("bad", &["backend"][..]),
    ] {
        let (o, lines, _) = fake_live_run(mode, false);
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(2), "{mode}: {err}");
        // A missing cost counts as the full per-run cap.
        assert!(
            err.contains("budget stop before run 3: spent 0.9000"),
            "{mode}: {err}"
        );
        assert_eq!(lines.len(), 3, "{mode}");
        for l in &lines {
            assert!(kinds.contains(&l["error_kind"].as_str().unwrap()), "{l}");
            assert_eq!(l["client_version"], "9.9.9 (fake)", "{l}");
            assert!(l["wall_ms"].is_u64(), "{l}");
        }
    }
}

#[cfg(unix)]
#[test]
fn live_backend_stops_after_a_call_over_the_per_run_cap() {
    let (o, lines, _) = fake_live_run("pricey", false);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(2), "{err}");
    assert!(err.contains("cost stop after run 0"), "{err}");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["cost_usd"], 0.5);
}

#[cfg(unix)]
#[test]
fn live_backend_counts_only_a_valid_reported_cost() {
    // --budget-usd 1.0, --per-run-usd 0.3. An invalid cost counts as 0.3, like a missing one.
    for (mode, stop) in [
        ("cost-0.0", None),
        ("cost-0.25", Some("budget stop before run 3: spent 0.7500")),
        ("cost--1.0", Some("budget stop before run 3: spent 0.9000")),
        // NaN is not JSON, so the reply is malformed and has no cost.
        ("cost-NaN", Some("budget stop before run 3: spent 0.9000")),
    ] {
        let (o, lines, _) = fake_live_run(mode, false);
        let err = String::from_utf8_lossy(&o.stderr);
        match stop {
            Some(msg) => {
                assert_eq!(o.status.code(), Some(2), "{mode}: {err}");
                assert!(err.contains(msg), "{mode}: {err}");
                assert_eq!(lines.len(), 3, "{mode}");
            }
            None => {
                assert!(o.status.success(), "{mode}: {err}");
                assert!(err.contains("spent 0.0000 USD"), "{mode}: {err}");
                assert_eq!(lines.len(), 73, "{mode}");
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn live_backend_refused_out_path_leaks_no_dir() {
    let (o, lines, out) = fake_live_run("ok", true);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("exists"));
    assert!(lines.is_empty());
    assert_eq!(std::fs::read_to_string(out).unwrap(), "");
}
