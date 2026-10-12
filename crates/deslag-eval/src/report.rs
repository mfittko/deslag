//! Markdown report over a results JSONL file. Pairs each arm run with the
//! baseline run of the same task; a run without its partner is not analysed.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Write,
};

use serde_json::Value;

use crate::{ARMS, score};

/// Fields summed in the per-arm table, space-separated: backend-reported,
/// then `wall_ms`, the harness wall clock around each backend call.
const REPORTED: &str = "input_tokens output_tokens cache_read_tokens cache_creation_tokens cost_usd duration_ms turns wall_ms";

macro_rules! cells {
    ($($x:expr),* $(,)?) => { vec![$($x.to_string()),*] };
}

/// Paired 2x2 counts for one arm against baseline.
#[derive(Default)]
struct Paired {
    n: u32,
    base_ok: u32,
    arm_ok: u32,
    /// Only the baseline succeeded.
    b: u32,
    /// Only the arm succeeded.
    c: u32,
    unchanged: u32,
}

impl Paired {
    fn add(&mut self, base: &Value, arm: &Value) {
        let (bs, ar) = (base["success"] == true, arm["success"] == true);
        self.n += 1;
        self.base_ok += u32::from(bs);
        self.arm_ok += u32::from(ar);
        self.b += u32::from(bs && !ar);
        self.c += u32::from(!bs && ar);
        self.unchanged += u32::from(arm["variant_unchanged"] == true);
    }

    /// Both success rates, difference and lower bound in pp, verdict. A
    /// `Paired` exists only after its first `add`, so `n > 0`.
    fn cells(&self) -> Vec<String> {
        let (n, b, c) = (self.n, self.b, self.c);
        let pp = |x: f64| format!("{:+.1}", x * 100.0);
        let d = pp((f64::from(c) - f64::from(b)) / f64::from(n));
        let (base, arm) = (rate(self.base_ok, n), rate(self.arm_ok, n));
        cells![
            base,
            arm,
            d,
            pp(score::lower_bound(n, b, c)),
            score::verdict(n, b, c)
        ]
    }
}

fn rate(k: u32, n: u32) -> String {
    if n == 0 {
        return "n/a".into();
    }
    format!("{k}/{n} ({:.1}%)", f64::from(k) * 100.0 / f64::from(n))
}

/// Appends a section with one table. `head` separates columns with `|`. A
/// non-empty `note` ends with a blank line.
fn section(md: &mut String, title: &str, note: &str, head: &str, rows: Vec<Vec<String>>) {
    let sep = " --- |".repeat(head.split('|').count());
    let _ = write!(md, "### {title}\n\n{note}");
    let _ = writeln!(md, "| {} |\n|{sep}", head.replace('|', " | "));
    for r in rows {
        let _ = writeln!(md, "| {} |", r.join(" | "));
    }
    md.push('\n');
}

/// Success, wrong, abstain and backend-error cells for `rows`.
fn kinds(rows: &[&Value]) -> Vec<String> {
    let n = rows.len() as u32;
    let count = |k: &str| rows.iter().filter(|r| r["error_kind"] == k).count();
    let ok = rows.iter().filter(|r| r["success"] == true).count() as u32;
    let (wrong, abstain, backend) = (count("wrong"), count("abstain"), count("backend"));
    cells![rate(ok, n), wrong, abstain, backend]
}

/// Sum of a reported numeric field, or `n/a` when no run reported it.
fn reported(rows: &[&Value], key: &str) -> String {
    let vals: Vec<f64> = rows.iter().filter_map(|r| r[key].as_f64()).collect();
    match (vals.is_empty(), key) {
        (true, _) => "n/a".into(),
        (false, "cost_usd") => format!("{:.4}", vals.iter().sum::<f64>()),
        (false, _) => vals.iter().sum::<f64>().to_string(),
    }
}

pub fn render(text: &str) -> Result<String, String> {
    let rows = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| {
            let v: Value = serde_json::from_str(l).map_err(|e| format!("line {}: {e}", i + 1))?;
            match ["task", "category", "arm", "error_kind"]
                .iter()
                .find(|k| !v[**k].is_string())
            {
                Some(k) => Err(format!("line {}: no {k}", i + 1)),
                None => Ok(v),
            }
        })
        .collect::<Result<Vec<Value>, String>>()?;
    let first = rows.first().ok_or("no result lines")?;
    // Runs pair within one seed, so concatenated seeds stay apart.
    let key = |r: &Value| {
        (
            r["seed"].to_string(),
            r["task"].as_str().unwrap_or("").to_string(),
        )
    };
    let base: HashMap<(String, String), &Value> = rows
        .iter()
        .filter(|r| r["arm"] == "baseline")
        .map(|r| (key(r), r))
        .collect();
    // Paired counts keyed by (scope, arm). Scope is "all", a category or "control:<kind>".
    let mut paired: BTreeMap<(String, String), Paired> = BTreeMap::new();
    let mut unpaired = 0;
    for r in rows.iter().filter(|r| r["arm"] != "baseline") {
        let Some(b) = base.get(&key(r)) else {
            unpaired += 1;
            continue;
        };
        let arm = r["arm"].as_str().unwrap_or("").to_string();
        let scopes = match r["control"].as_str() {
            Some(c) => vec![format!("control:{c}")],
            None => vec!["all".into(), r["category"].as_str().unwrap_or("").into()],
        };
        for s in scopes {
            paired.entry((s, arm.clone())).or_default().add(b, r);
        }
    }

    let mut md = String::new();
    let all: Vec<&Value> = rows.iter().collect();
    let seeds: BTreeSet<String> = rows.iter().map(|r| r["seed"].to_string()).collect();
    let meta = |k: &str| first[k].as_str().unwrap_or("n/a").to_string();
    let run = vec![
        cells!["backend", meta("backend")],
        cells!["model", meta("model")],
        cells!["client version", meta("client_version")],
        cells!["seed", seeds.into_iter().collect::<Vec<_>>().join(", ")],
        cells!["result lines", rows.len()],
        cells!["runs without a baseline partner", unpaired],
        cells!["reported cost, USD", reported(&all, "cost_usd")],
    ];
    section(&mut md, "Run", "", "Field|Value", run);

    let scoped = |pred: &dyn Fn(&str) -> bool, with_scope: bool| -> Vec<Vec<String>> {
        let rows = paired.iter().filter(|((s, _), _)| pred(s));
        rows.map(|((s, arm), p)| {
            let head = if with_scope {
                cells![s, arm, p.n]
            } else {
                cells![arm, p.n, p.unchanged, p.b, p.c]
            };
            [head, p.cells()].concat()
        })
        .collect()
    };
    let note = "Non-control tasks. Agresti-Min lower one-sided 95% bound; an arm passes when the bound exceeds -2.0 pp and is inferior when the upper one-sided 95% bound is below -2.0 pp. b: only baseline succeeded; c: only the arm succeeded.\n\n";
    let head =
        "Arm|Pairs|Unchanged|b|c|Baseline success|Arm success|Difference pp|Lower bound pp|Verdict";
    let per_arm = scoped(&|s| s == "all", false);
    section(&mut md, "Non-inferiority per arm", note, head, per_arm);
    let head =
        "Category|Arm|Pairs|Baseline success|Arm success|Difference pp|Lower bound pp|Verdict";
    let per_category = scoped(&|s| s != "all" && !s.starts_with("control:"), true);
    section(&mut md, "Per category", "", head, per_category);

    // Non-control tasks that ran both table formats in the same seed, so both
    // rows score the same inputs.
    let has =
        |k: &(String, String), arm: &str| rows.iter().any(|r| key(r) == *k && r["arm"] == arm);
    let formats = [("table-v1 CSV", "table-csv"), ("Markdown-KV", "table-mdkv")];
    let both = |r: &&Value| {
        r["control"].is_null() && has(&key(r), "table-csv") && has(&key(r), "table-mdkv")
    };
    let format_rows = formats.iter().map(|(label, arm)| {
        let rs: Vec<&Value> = rows
            .iter()
            .filter(|r| r["arm"] == *arm)
            .filter(both)
            .collect();
        [cells![label, arm, rs.len()], kinds(&rs)].concat()
    });
    let note = "Non-control tasks that ran both table arms in the same seed. Control tasks are excluded.\n\n";
    let head = "Format|Arm|Runs|Success|Wrong|Abstain|Backend error";
    let format_rows = format_rows.collect();
    section(&mut md, "Table format read-back", note, head, format_rows);

    let mut controls = Vec::new();
    for ((scope, arm), p) in &paired {
        let Some(control) = scope.strip_prefix("control:") else {
            continue;
        };
        let result = match control {
            "incompressible" if p.unchanged == p.n => "all variants unchanged",
            "incompressible" => "a transform changed a control input",
            "evidence_drop" if *arm != "drop-evidence" && p.b == 0 && p.base_ok > 0 => {
                "evidence kept"
            }
            "evidence_drop" if *arm != "drop-evidence" && p.base_ok == 0 => "baseline failed",
            "evidence_drop" if *arm != "drop-evidence" => "a transform lost evidence",
            "evidence_drop" if p.arm_ok < p.base_ok => "evidence loss detected",
            "evidence_drop" => "evidence loss not detected",
            _ => "unknown control",
        };
        let (base_ok, arm_ok) = (rate(p.base_ok, p.n), rate(p.arm_ok, p.n));
        let counts = cells![p.n, base_ok, arm_ok, p.unchanged, p.b + p.c];
        controls.push([cells![control, arm], counts, cells![result]].concat());
    }
    let head = "Control|Arm|Pairs|Baseline success|Arm success|Unchanged|Discordant|Result";
    section(&mut md, "Controls", "", head, controls);

    let per_arm = std::iter::once("baseline").chain(ARMS).filter_map(|arm| {
        let rs: Vec<&Value> = rows.iter().filter(|r| r["arm"] == arm).collect();
        let est: u64 = rs
            .iter()
            .filter_map(|r| r["payload_tokens_est"].as_u64())
            .sum();
        let usage = REPORTED.split(' ').map(|k| reported(&rs, k)).collect();
        let head = cells![arm, rs.len()];
        (!rs.is_empty()).then(|| [head, kinds(&rs), cells![est], usage].concat())
    });
    let note = "All tasks, controls included. Payload tokens are estimates (deslag-estimate-v1). The columns from Input tokens to Turns are backend-reported sums; n/a means not reported. Wall ms is the summed harness wall clock of the backend calls.\n\n";
    let head = "Arm|Runs|Success|Wrong|Abstain|Backend error|Payload tokens (estimate)|Input tokens|Output tokens|Cache read|Cache creation|Cost USD|Duration ms|Turns|Wall ms";
    section(&mut md, "Per arm", note, head, per_arm.collect());
    Ok(md.trim_end().to_string() + "\n")
}
