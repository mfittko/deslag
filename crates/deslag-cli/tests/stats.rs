use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use deslag_proxy::session::{Owner, write_owner};
use serde_json::Value;

/// Ten minutes into the hour that starts at 1_800_000_000.
const NOW: &str = "1800000600";

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/../../tests/fixtures/stats/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    fs::read_to_string(path).unwrap()
}

/// A fresh short state dir under /tmp holding `stats` as `stats.jsonl`.
fn state_dir(name: &str, stats: Option<&str>) -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/dsl-st-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    if let Some(text) = stats {
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("stats.jsonl"), text).unwrap();
    }
    dir
}

fn deslag(dir: &Path, now: Option<&str>, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_deslag"));
    cmd.args(args).env("DESLAG_STATE_DIR", dir);
    if let Some(now) = now {
        cmd.env("DESLAG_NOW", now);
    }
    cmd.output().expect("run deslag")
}

fn stdout(output: &Output) -> String {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stats_json(dir: &Path, args: &[&str]) -> Value {
    let args = [&["stats", "--json"][..], args].concat();
    serde_json::from_str(&stdout(&deslag(dir, Some(NOW), &args))).unwrap()
}

#[test]
fn json_output_matches_the_snapshot() {
    let dir = state_dir("snap", Some(&fixture("stats.jsonl")));
    let output = deslag(&dir, Some(NOW), &["stats", "--json", "--since", "2h"]);
    assert_eq!(stdout(&output), fixture("since-2h.json"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn since_selects_the_window() {
    let dir = state_dir("since", Some(&fixture("stats.jsonl")));
    for (args, since, reports) in [
        (&[][..], 1_800_000_000 - 24 * 3600, 6),
        (&["--since", "1h"], 1_800_000_000 - 3600, 3),
        (&["--since", "2d"], 1_800_000_000 - 48 * 3600, 11),
    ] {
        let stats = stats_json(&dir, args);
        assert_eq!(stats["window"]["since"], since, "{args:?}");
        assert_eq!(stats["window"]["until"], 1_800_000_600u64);
        assert_eq!(stats["totals"]["reports"], reports, "{args:?}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn observe_proposals_stay_separate_from_emitted_reductions() {
    let dir = state_dir("observe", Some(&fixture("stats.jsonl")));
    let totals = &stats_json(&dir, &["--since", "2h"])["totals"];
    assert_eq!(totals["emitted_reduction_bytes"], 400);
    assert_eq!(totals["would_change_reduction_bytes"], 200);
    let table = stdout(&deslag(&dir, Some(NOW), &["stats", "--since", "2h"]));
    let row = |outcome: &str| -> Vec<String> {
        let line = table
            .lines()
            .find(|l| l.split_whitespace().nth(2) == Some(outcome))
            .unwrap_or_else(|| panic!("no {outcome} row in\n{table}"));
        line.split_whitespace().map(String::from).collect()
    };
    // count, original, emitted, saved bytes, saved tokens
    assert_eq!(row("changed")[5..], ["2", "1000", "600", "400", "100"]);
    assert_eq!(row("would_change")[5..], ["1", "500", "500", "200", "-"]);
    assert_eq!(row("skipped")[3], "below_min_bytes");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unknown_values_are_null_never_zero() {
    let line = r#"{"v":1,"hour":1799996400,"adapter":"proxy","transform":"json-minify","outcome":"would_change","reason":null,"tool":"bash","count":1,"original_bytes":500,"candidate_bytes":300,"emitted_bytes":500,"tokens":{},"dropped":0}"#;
    // A null would_change candidate is malformed; merged, it would overstate the proposal.
    let unknown = line.replace("300", "null");
    let dir = state_dir("null", Some(&format!("{line}\n{unknown}\n")));
    let stats = stats_json(&dir, &["--since", "1h"]);
    assert_eq!(stats["totals"]["reports"], 1);
    assert_eq!(stats["totals"]["would_change_reduction_bytes"], 200);
    assert_eq!(stats["totals"]["emitted_reduction_bytes"], 0);
    assert_eq!(stats["buckets"][0]["tokens"], serde_json::json!({}));
    let table = stdout(&deslag(&dir, Some(NOW), &["stats", "--since", "1h"]));
    let row = table.lines().find(|l| l.contains("would_change")).unwrap();
    let saved: Vec<&str> = row.split_whitespace().skip(8).collect();
    assert_eq!(saved, ["200", "-"], "{row}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn default_output_is_a_table() {
    let dir = state_dir("table", Some(&fixture("stats.jsonl")));
    let table = stdout(&deslag(&dir, Some(NOW), &["stats"]));
    assert!(serde_json::from_str::<Value>(&table).is_err());
    let mut lines = table.lines();
    assert_eq!(
        lines.next().unwrap().split_whitespace().collect::<Vec<_>>(),
        [
            "adapter",
            "transform",
            "outcome",
            "reason",
            "tool",
            "count",
            "original",
            "emitted",
            "saved",
            "tokens_saved"
        ]
    );
    assert!(table.contains("window: last 24h"), "{table}");
    assert!(
        table.contains("partial: service_not_running, drops, gaps"),
        "{table}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn partial_reasons_report_gaps_drops_and_missing_data() {
    let reasons = |dir: &Path, args: &[&str]| stats_json(dir, args)["partial"].clone();
    let fixture = fixture("stats.jsonl");

    let empty = state_dir("empty", None);
    assert_eq!(
        reasons(&empty, &[]),
        serde_json::json!({"value": true, "reasons": ["service_not_running", "gaps", "no_data"]})
    );

    // Without the drops and with the service line of 1799996400 removed.
    let gappy: String = fixture
        .lines()
        .filter(|l| !l.contains(r#""dropped":2"#) && !l.contains(r#""dropped":1"#))
        .map(|l| format!("{l}\n"))
        .collect();
    let dir = state_dir("gaps", Some(&gappy));
    assert_eq!(
        reasons(&dir, &["--since", "2h"])["reasons"],
        serde_json::json!(["service_not_running", "gaps"])
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Stops the service on drop, so a failing assertion never orphans it.
struct StopOnDrop(PathBuf);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        let _ = deslag(&self.0, None, &["local", "stop"]);
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_running_service_flushes_the_current_hour() {
    let dir = state_dir("live", None);
    let _guard = StopOnDrop(dir.clone());
    // The hook compresses, and so reports, only when it owns compression.
    write_owner(&dir, Owner::Hook).unwrap();
    assert!(stdout(&deslag(&dir, None, &["local", "start"])).contains("started"));
    let hook = format!(
        "{}/../../tests/fixtures/claude/posttooluse-bash-json.json",
        env!("CARGO_MANIFEST_DIR")
    );
    // Keep the developer's real Claude Code settings out of hook conflict checks.
    let output = Command::new(env!("CARGO_BIN_EXE_deslag"))
        .args(["hook", "claude", "--minify-json"])
        .env("DESLAG_STATE_DIR", &dir)
        .env("CLAUDE_CONFIG_DIR", &dir)
        .env("CLAUDE_PROJECT_DIR", &dir)
        .env("HOME", &dir)
        .stdin(fs::File::open(hook).unwrap())
        .output()
        .unwrap();
    assert!(!output.stdout.is_empty());

    let output = deslag(&dir, None, &["stats", "--json", "--since", "1h"]);
    let stats: Value = serde_json::from_str(&stdout(&output)).unwrap();
    let reasons = stats["partial"]["reasons"].as_array().unwrap();
    assert!(!reasons.contains(&"service_not_running".into()), "{stats}");
    assert_eq!(stats["totals"]["reports"], 1, "{stats}");
    assert_eq!(stats["buckets"][0]["outcome"], "changed");
}

#[test]
fn an_unreadable_stats_file_exits_2_and_a_missing_one_exits_0() {
    let dir = state_dir("unreadable", None);
    fs::create_dir_all(&dir).unwrap();
    assert_eq!(deslag(&dir, Some(NOW), &["stats"]).status.code(), Some(0));
    fs::create_dir(dir.join("stats.jsonl")).unwrap();
    let output = deslag(&dir, Some(NOW), &["stats"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read stats.jsonl"));
}

#[test]
fn bad_stats_arguments_print_usage_and_exit_64() {
    let dir = state_dir("usage", None);
    for args in [
        &["stats", "--since"][..],
        &["stats", "--since", "0h"],
        &["stats", "--since", "5m"],
        &["stats", "--since", "xh"],
        &["stats", "--since", "-1d"],
        &["stats", "--since", "+2h"],
        &["stats", "table"],
    ] {
        let output = deslag(&dir, Some(NOW), args);
        assert_eq!(output.status.code(), Some(64), "{args:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage: deslag stats"));
    }
}

#[test]
fn huge_sums_saturate_instead_of_panicking() {
    let line = format!(
        "{{\"v\":1,\"hour\":1800000000,\"adapter\":\"hook\",\"transform\":\"json-minify\",\
         \"outcome\":\"changed\",\"reason\":null,\"tool\":\"bash\",\"count\":{max},\
         \"original_bytes\":{max},\"candidate_bytes\":0,\"emitted_bytes\":0,\"tokens\":{{}},\
         \"dropped\":0}}\n",
        max = u64::MAX
    );
    let other_tool = line.replace("\"bash\"", "\"mcp\"");
    let dir = state_dir("huge", Some(&(line + &other_tool)));
    let totals = &stats_json(&dir, &["--since", "1h"])["totals"];
    assert_eq!(totals["emitted_reduction_bytes"], u64::MAX);
    assert_eq!(totals["reports"], u64::MAX);
}
