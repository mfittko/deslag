//! The paired-savings report section over a fixture results file. Offline only.

use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_deslag-eval");

fn eval(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_remove("DESLAG_EVAL_LIVE")
        .output()
        .unwrap()
}

fn fresh_dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn result(task: &str, arm: &str, control: Value, est: u64, usage: [Option<i64>; 3]) -> String {
    let mut line = json!({
        "task": task, "category": "api_query", "control": control, "arm": arm,
        "seed": 1, "backend": "claude", "model": "m", "client_version": "v",
        "variant_unchanged": false, "payload_tokens_est": est,
        "success": true, "error_kind": "none",
    });
    for (key, v) in ["input_tokens", "cache_read_tokens", "cache_creation_tokens"]
        .iter()
        .zip(usage)
    {
        line[*key] = json!(v);
    }
    format!("{line}\n")
}

#[test]
fn report_pairs_savings_per_arm() {
    let u = |a: i64, b, c| [Some(a), Some(b), Some(c)];
    let none = [None; 3];
    let mut text = String::new();
    // json-minify: two pairs with Claude counts, one whose baseline lacks cache creation, one control.
    for (task, arm, est, usage) in [
        ("t1", "baseline", 40, u(100, 0, 50)),
        ("t1", "json-minify", 25, u(80, 0, 20)),
        ("t2", "baseline", 60, u(200, 10, 0)),
        ("t2", "json-minify", 30, u(120, 10, 0)),
        ("t3", "baseline", 20, [Some(5), Some(5), None]),
        ("t3", "json-minify", 10, u(1, 1, 1)),
        ("t4", "baseline", 10, u(100, 0, 0)),
        ("t4", "lines-v1", 10, u(100, 0, 0)),
        ("t5", "baseline", 50, none),
        ("t5", "table-csv", 20, none),
    ] {
        text += &result(task, arm, Value::Null, est, usage);
    }
    // table-v2: negative Claude saving (-50 on a baseline of 100), estimate saves 20; a bad estimate drops t7.
    for (task, arm, est, usage) in [
        ("t6", "baseline", 40, u(100, 0, 0)),
        ("t6", "table-v2", 20, u(150, 0, 0)),
        ("t7", "baseline", 40, u(1, 0, 0)),
        ("t7", "table-v2", 0, u(1, 0, 0)),
        ("t8", "baseline", 40, u(100, 0, 0)),
        ("t8", "lines-v2", 10, u(-1, 0, 0)),
    ] {
        let mut line = result(task, arm, Value::Null, est, usage);
        if task == "t7" && arm == "table-v2" {
            line = line.replace("\"payload_tokens_est\":0", "\"payload_tokens_est\":null");
        }
        text += &line;
    }
    // Only a drop-evidence control with baseline partner reporting usage; and an arm row without partner.
    for arm in ["baseline", "drop-evidence", "json-minify"] {
        text += &result("e1", arm, json!("evidence_drop"), 7, u(1, 1, 1));
    }
    text += &result("t9", "table-mdkv", Value::Null, 5, none);
    for arm in ["baseline", "json-minify"] {
        text += &result("c1", arm, json!("incompressible"), 9, u(1, 1, 1));
    }
    let path = fresh_dir("savings").join("results.jsonl");
    std::fs::write(&path, text).unwrap();
    let o = eval(&["report", path.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let md = String::from_utf8(o.stdout).unwrap();
    let section = md.split("### Paired savings").nth(1).unwrap();
    for row in [
        // Estimate saved 55 of 120; Claude saved 130 of 360 on the two complete pairs; (45 - 130) / 130.
        "| json-minify | 3 | 55 | 45.8 | 2 | 360 | 230 | 130 | 36.1 | -65.4 |",
        // Zero Claude savings: the percentage is 0.0 and the estimate error is undefined.
        "| lines-v1 | 1 | 0 | 0.0 | 1 | 100 | 100 | 0 | 0.0 | n/a |",
        "| table-csv | 1 | 30 | 60.0 | 0 | n/a | n/a | n/a | n/a | n/a |",
        // Negative Claude saving: (20 - -50) / |-50| = 140.0. t7 has an invalid estimate and is dropped.
        "| table-v2 | 1 | 20 | 50.0 | 1 | 100 | 150 | -50 | -50.0 | 140.0 |",
        // input_tokens -1 is not a Claude count.
        "| lines-v2 | 1 | 30 | 75.0 | 0 | n/a | n/a | n/a | n/a | n/a |",
        // No baseline partner: both estimate columns are n/a.
        "| table-mdkv | 0 | n/a | n/a | 0 | n/a | n/a | n/a | n/a | n/a |",
    ] {
        assert!(section.contains(row), "{row}\n{section}");
    }
    assert!(!section.contains("| drop-evidence |"), "{section}");
}
