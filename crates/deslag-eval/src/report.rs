//! Markdown report over a results JSONL file. Pairs each arm run with the
//! baseline run of the same task; a run without its partner is not analysed.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Write,
};

use serde_json::Value;

use crate::{ARMS, score};

/// Fields summed in the per-arm table, space-separated: backend-reported,
/// then `wall_ms`, the harness wall clock around each backend call.
const REPORTED: &str = "input_tokens output_tokens cache_read_tokens cache_creation_tokens cost_usd duration_ms turns wall_ms";

/// Fields every result line must carry as strings.
const REQUIRED: [&str; 4] = ["task", "category", "arm", "error_kind"];

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

    /// Both success rates, difference and lower bound in pp, verdict. With
    /// `n == 0` the difference and bound are `n/a` and the verdict is `no data`.
    fn cells(&self) -> Vec<String> {
        let (n, b, c) = (self.n, self.b, self.c);
        let pp = |x: f64| format!("{:+.1}", x * 100.0);
        let (diff, low) = match n {
            0 => ("n/a".to_string(), "n/a".to_string()),
            _ => (
                pp((f64::from(c) - f64::from(b)) / f64::from(n)),
                pp(score::lower_bound(n, b, c)),
            ),
        };
        cells![
            rate(self.base_ok, n),
            rate(self.arm_ok, n),
            diff,
            low,
            score::verdict(n, b, c)
        ]
    }
}

fn rate(k: u32, n: u32) -> String {
    match n {
        0 => "n/a".into(),
        _ => format!("{k}/{n} ({:.1}%)", f64::from(k) * 100.0 / f64::from(n)),
    }
}

/// Appends a section with one table. `head` separates columns with `|`. A
/// non-empty `note` ends with a blank line.
fn section(md: &mut String, title: &str, note: &str, head: &str, rows: Vec<Vec<String>>) {
    let sep = " --- |".repeat(head.split('|').count());
    let _ = write!(md, "### {title}\n\n{note}");
    let _ = writeln!(md, "| {} |\n|{sep}", head.replace('|', " | "));
    // Cells may carry result-file or client text, so `|` and line breaks cannot shift columns.
    let escape = |c: &String| c.replace('|', "\\|").replace(['\r', '\n'], " ");
    for r in rows {
        let r: Vec<String> = r.iter().map(escape).collect();
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
/// A negative or non-finite value counts as unreported, as in the run budget.
fn reported(rows: &[&Value], key: &str) -> String {
    let vals: Vec<f64> = rows
        .iter()
        .filter_map(|r| r[key].as_f64())
        .filter(|x| x.is_finite() && *x >= 0.0)
        .collect();
    match (vals.is_empty(), key) {
        (true, _) => "n/a".into(),
        (false, "cost_usd") => format!("{:.4}", vals.iter().sum::<f64>()),
        (false, _) => vals.iter().sum::<f64>().to_string(),
    }
}

/// Backend-reported prompt tokens of a run: input plus cache read plus cache
/// creation. `None` unless all three are finite numbers >= 0.
fn prompt_tokens(r: &Value) -> Option<f64> {
    ["input_tokens", "cache_read_tokens", "cache_creation_tokens"]
        .iter()
        .map(|k| r[*k].as_f64().filter(|x| x.is_finite() && *x >= 0.0))
        .sum()
}

pub fn render(text: &str) -> Result<String, String> {
    let rows = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| {
            let v: Value = serde_json::from_str(l).map_err(|e| format!("line {}: {e}", i + 1))?;
            match REQUIRED.iter().find(|k| !v[**k].is_string()) {
                Some(k) => Err(format!("line {}: no {k}", i + 1)),
                None => Ok(v),
            }
        })
        .collect::<Result<Vec<Value>, String>>()?;
    let first = rows.first().ok_or("no result lines")?;
    // One task is one paired observation, so a file holds one run: one seed,
    // backend and model, and each (task, arm) at most once.
    let mut seen = HashSet::new();
    for (i, r) in rows.iter().enumerate() {
        if ["seed", "backend", "model"]
            .iter()
            .any(|k| r[*k] != first[*k])
        {
            return Err(format!(
                "line {}: seed, backend or model differs from line 1; report one run per file",
                i + 1
            ));
        }
        if !seen.insert((r["task"].as_str(), r["arm"].as_str())) {
            return Err(format!(
                "line {}: duplicate task {} arm {}",
                i + 1,
                r["task"],
                r["arm"]
            ));
        }
    }
    let key = |r: &Value| r["task"].to_string();
    let base: HashMap<String, &Value> = rows
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
    // An arm without non-control pairs still gets a per-arm row with verdict
    // `no data`. drop-evidence is a control arm and gets no per-arm row.
    for r in &rows {
        let arm = r["arm"].as_str().unwrap_or("");
        if arm != "baseline" && arm != "drop-evidence" {
            paired.entry(("all".into(), arm.into())).or_default();
        }
    }

    let mut md = String::new();
    let all: Vec<&Value> = rows.iter().collect();
    let meta = |k: &str| first[k].as_str().unwrap_or("n/a").to_string();
    let run = vec![
        cells!["backend", meta("backend")],
        cells!["model", meta("model")],
        cells!["client version", meta("client_version")],
        cells!["seed", first["seed"]],
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

    // Non-control tasks that ran both table formats, so both rows score the
    // same inputs.
    let has = |r: &Value, arm: &str| rows.iter().any(|x| key(x) == key(r) && x["arm"] == arm);
    let formats = [("table-v1 CSV", "table-csv"), ("Markdown-KV", "table-mdkv")];
    let both = |r: &Value| r["control"].is_null() && has(r, "table-csv") && has(r, "table-mdkv");
    let format_rows = formats.iter().map(|(label, arm)| {
        let rs: Vec<&Value> = rows
            .iter()
            .filter(|r| r["arm"] == *arm && both(r))
            .collect();
        [cells![label, arm, rs.len()], kinds(&rs)].concat()
    });
    let note = "Non-control tasks that ran both table arms. Control tasks are excluded.\n\n";
    let head = "Format|Arm|Runs|Success|Wrong|Abstain|Backend error";
    let format_rows = format_rows.collect();
    section(&mut md, "Table format read-back", note, head, format_rows);

    let mut controls = Vec::new();
    for ((scope, arm), p) in &paired {
        let Some(control) = scope.strip_prefix("control:") else {
            continue;
        };
        let transform = *arm != "drop-evidence";
        let result = match control {
            "incompressible" if p.unchanged == p.n => "all variants unchanged",
            "incompressible" => "a transform changed a control input",
            "evidence_drop" if transform && p.b == 0 && p.base_ok > 0 => "evidence kept",
            "evidence_drop" if transform && p.base_ok == 0 => "baseline failed",
            "evidence_drop" if transform => "a transform lost evidence",
            "evidence_drop" if p.base_ok == 0 => "baseline failed",
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
        (!rs.is_empty()).then(|| [cells![arm, rs.len()], kinds(&rs), cells![est], usage].concat())
    });
    let note = "All tasks, controls included. Payload tokens are estimates (deslag-estimate-v1). The columns from Input tokens to Turns are backend-reported sums; n/a means not reported, and a negative or non-finite value counts as not reported. Wall ms is the summed harness wall clock of the backend calls.\n\n";
    let head = "Arm|Runs|Success|Wrong|Abstain|Backend error|Payload tokens (estimate)|Input tokens|Output tokens|Cache read|Cache creation|Cost USD|Duration ms|Turns|Wall ms";
    section(&mut md, "Per arm", note, head, per_arm.collect());

    let valid = |r: &Value, k: &str| r[k].as_f64().filter(|x| x.is_finite() && *x >= 0.0);
    let est = |r: &Value| valid(r, "payload_tokens_est").unwrap_or(0.0);
    let pct = |part: f64, whole: f64| match whole {
        0.0 => "n/a".to_string(),
        _ => format!("{:.1}", part / whole * 100.0),
    };
    let savings = paired.keys().filter(|(s, _)| s == "all").map(|(_, arm)| {
        let pairs: Vec<(&Value, &Value)> = rows
            .iter()
            .filter(|r| r["arm"] == arm.as_str() && r["control"].is_null())
            .filter_map(|r| base.get(&key(r)).map(|b| (*b, r)))
            .filter(|(b, r)| {
                [b, r]
                    .iter()
                    .all(|x| valid(x, "payload_tokens_est").is_some())
            })
            .collect();
        let sum = |f: &dyn Fn(&(&Value, &Value)) -> f64| pairs.iter().map(f).sum::<f64>();
        let (est_base, est_saved) = (sum(&|p| est(p.0)), sum(&|p| est(p.0) - est(p.1)));
        let est_saved_cell = if pairs.is_empty() {
            "n/a".into()
        } else {
            est_saved.to_string()
        };
        // (baseline, arm, baseline estimate, arm estimate) of pairs with Claude counts on both runs.
        let claude: Vec<[f64; 4]> = pairs
            .iter()
            .filter_map(|(b, r)| Some([prompt_tokens(b)?, prompt_tokens(r)?, est(b), est(r)]))
            .collect();
        let col = |i: usize| claude.iter().map(|c| c[i]).sum::<f64>();
        let (base_tok, saved) = (col(0), col(0) - col(1));
        let na = || "n/a".to_string();
        let (arm_tok, saved_tok, saved_pct, error) = if claude.is_empty() {
            (na(), na(), na(), na())
        } else {
            let error = if saved == 0.0 {
                na()
            } else {
                pct(col(2) - col(3) - saved, saved.abs())
            };
            (
                col(1).to_string(),
                saved.to_string(),
                pct(saved, base_tok),
                error,
            )
        };
        let base_tok = if claude.is_empty() {
            na()
        } else {
            base_tok.to_string()
        };
        let head = cells![
            arm,
            pairs.len(),
            est_saved_cell,
            pct(est_saved, est_base),
            claude.len()
        ];
        [head, cells![base_tok, arm_tok, saved_tok, saved_pct, error]].concat()
    });
    let note = "Secondary measure; it never decides pass or fail. Non-control tasks; drop-evidence is a control arm and has no row. Saved is baseline minus arm, summed over the pairs. The estimate columns sum payload tokens (deslag-estimate-v1) over all pairs. The Claude columns sum backend-reported input, cache read and cache creation tokens over the pairs where both runs report all three; they include the system prompt and client overhead, which cancel in the difference but dilute the percentage. Estimate error % is the estimate saved on those same pairs minus the Claude saved, over the absolute Claude saved; positive means the estimate overstates the saving. A pair whose baseline or arm payload estimate is not a finite number of at least 0 is dropped from the whole row.\n\n";
    let head = "Arm|Pairs|Saved tokens (estimate)|Saved % (estimate)|Pairs with Claude tokens|Baseline prompt tokens (Claude)|Arm prompt tokens (Claude)|Saved tokens (Claude)|Saved % (Claude)|Estimate error %";
    section(&mut md, "Paired savings", note, head, savings.collect());
    Ok(md.trim_end().to_string() + "\n")
}
