//! Generator, self-check, manifest include and the paired-savings report.
//! Offline only; no test reaches a paid model.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use deslag_eval::generate::{Pick, Query, check};
use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_deslag-eval");

fn eval(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_remove("DESLAG_EVAL_LIVE")
        .output()
        .unwrap()
}

fn evaluations() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/evaluations")
}

fn fresh_dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Every file under `dir`, keyed by its path relative to `dir`.
fn files(dir: &Path, rel: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for e in std::fs::read_dir(dir.join(rel)).unwrap() {
        let path = rel.join(e.unwrap().file_name());
        if dir.join(&path).is_dir() {
            files(dir, &path, out);
        } else {
            out.insert(path.clone(), std::fs::read(dir.join(&path)).unwrap());
        }
    }
}

#[test]
fn regenerating_the_committed_corpus_is_byte_identical() {
    let committed = evaluations().join("generated");
    let doc: Value =
        serde_json::from_str(&std::fs::read_to_string(committed.join("manifest.json")).unwrap())
            .unwrap();
    let seed = doc["seed"].as_u64().unwrap().to_string();
    let out = fresh_dir("regenerated");
    let o = eval(&["generate", "--seed", &seed, "--out", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let (mut a, mut b) = (BTreeMap::new(), BTreeMap::new());
    files(&committed, Path::new(""), &mut a);
    files(&out, Path::new(""), &mut b);
    assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
    for (path, bytes) in &a {
        assert!(bytes == &b[path], "{} differs", path.display());
    }
}

#[test]
fn another_seed_changes_the_corpus() {
    let (a, b) = (fresh_dir("seed-a"), fresh_dir("seed-b"));
    for (seed, out) in [("1", &a), ("2", &b)] {
        let o = eval(&["generate", "--seed", seed, "--out", out.to_str().unwrap()]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }
    let read = |d: &Path| std::fs::read(d.join("inputs/gen-json-000.json")).unwrap();
    assert_ne!(read(&a), read(&b));
}

fn lookup(name: &str) -> Query {
    Query::Json {
        conds: vec![("name", json!(name))],
        pick: Pick::Field("id"),
    }
}

#[test]
fn self_check_refuses_an_answer_that_is_absent_wrong_or_not_unique() {
    let rows = r#"[{"id":"a1","name":"x","n":1204.10},{"id":"b2","name":"y","n":7},{"id":"c3","name":"y","n":7}]"#;
    assert_eq!(check(rows, &lookup("x"), "a1"), Ok(()));
    // Wrong answer, answer absent from the payload, no match, two matches.
    assert!(check(rows, &lookup("x"), "b2").is_err());
    assert!(check(rows, &lookup("x"), "zz").is_err());
    assert!(check(rows, &lookup("w"), "a1").is_err());
    assert!(check(rows, &lookup("y"), "b2").is_err());
    // A set needs every item verbatim in the payload and at least two matches.
    let set = |name| Query::Json {
        conds: vec![("name", json!(name))],
        pick: Pick::Set("id"),
    };
    assert_eq!(check(rows, &set("y"), "b2,c3"), Ok(()));
    assert!(check(rows, &set("y"), "b2,zz").is_err());
    assert!(check(rows, &set("x"), "a1").is_err());
    // A maximum must be unique; a number answer keeps its lexeme in the payload.
    let max = |name| Query::Json {
        conds: vec![],
        pick: Pick::Max { num: "n", name },
    };
    assert_eq!(check(rows, &max(None), "1204.10"), Ok(()));
    assert_eq!(check(rows, &max(Some("id")), "a1"), Ok(()));
    assert!(check(&rows.replace("1204.10", "7"), &max(Some("id")), "a1").is_err());
    // A text query matches exactly one line.
    let scan = Query::Scan {
        contains: "FAILED".into(),
        after: "test ".into(),
        end: " ",
    };
    assert_eq!(
        check("test a ... ok\ntest b::c ... FAILED\n", &scan, "b::c"),
        Ok(())
    );
    assert!(check("test a ... FAILED\ntest b ... FAILED\n", &scan, "a").is_err());
}

/// Writes a one-task manifest with `extra` top-level members and returns its path.
fn manifest_with(dir: &Path, name: &str, id: &str, extra: &str) -> PathBuf {
    let input = evaluations().join("inputs/api_query_repo.json");
    let text = format!(
        r#"{{{extra}"tasks": [{{"id": "{id}", "category": "api_query", "input": {:?}, "question": "q", "answers": ["trunk-2026"], "rule": "exact", "arms": ["json-minify"]}}]}}"#,
        input.to_str().unwrap()
    );
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn include_loads_one_level_and_refuses_nesting_and_duplicate_ids() {
    let dir = fresh_dir("includes");
    manifest_with(&dir, "inner.json", "b", "");
    manifest_with(&dir, "nested.json", "c", r#""include": ["inner.json"], "#);
    let top = manifest_with(&dir, "top.json", "a", r#""include": ["inner.json"], "#);
    let o = eval(&["variants", top.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let table = String::from_utf8(o.stdout).unwrap();
    assert!(table.contains("| a | json-minify |") && table.contains("| b | json-minify |"));
    let top = manifest_with(&dir, "top2.json", "a", r#""include": ["nested.json"], "#);
    let o = eval(&["variants", top.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("cannot include"));
    let top = manifest_with(&dir, "top3.json", "b", r#""include": ["inner.json"], "#);
    let o = eval(&["variants", top.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("duplicate id"));
}
