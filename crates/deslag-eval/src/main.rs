//! `deslag-eval`: paired task evaluation of deslag transforms. Development
//! only; the installed product does not depend on it. The scoring rules and the
//! criterion are predeclared in `docs/benchmark-methodology.md`.

use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use deslag_core::{
    CompressionContext, EstimateCounter, FIRST_SLICE_JSON_MINIFY_CONTEXT, TableOptions,
    TokenCounter, compress, minify_json, table,
};
use serde_json::{Value, json};

use deslag_eval::{
    ARMS, report,
    score::{self, ErrorKind, Rule},
};

const USAGE: &str = "usage:
  deslag-eval variants <manifest>
  deslag-eval run --manifest <path> --backend mock|claude --seed <u64> --out <results.jsonl>
                  [--model <id>] [--budget-usd <f>] [--per-run-usd <f>]
  deslag-eval report <results.jsonl>
  deslag-eval estimate --manifest <path> [--input-usd-per-mtok <f>] [--output-usd-per-mtok <f>]
                  [--overhead-tokens <n>] [--output-tokens <n>] [--per-run-usd <f>]

The claude backend also needs DESLAG_EVAL_LIVE=1, --model and --budget-usd.";

const SYSTEM_PROMPT: &str = "You answer questions about a tool result. Use only the tool result. Reply with the answer only, on one line, with no explanation. If the tool result does not contain the answer, reply NOT_FOUND.";

const DEFAULT_PER_RUN_USD: f64 = 0.30;

struct Task {
    id: String,
    category: String,
    control: Option<String>,
    input: String,
    question: String,
    answers: Vec<String>,
    rule: Rule,
    tolerance: f64,
    arms: Vec<String>,
    evidence: Option<String>,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("variants") if args.len() == 2 => variants(Path::new(&args[1])),
        Some("report") if args.len() == 2 => read(Path::new(&args[1]))
            .and_then(|text| report::render(&text))
            .map(|md| print!("{md}")),
        Some("run") => run(&args[1..]),
        Some("estimate") => estimate(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("deslag-eval: {e}");
            ExitCode::from(2)
        }
    }
}

/// Parses `--name value` pairs, refusing names not in the space-separated `allowed`.
fn flags(args: &[String], allowed: &str) -> Result<HashMap<String, String>, String> {
    let mut out = HashMap::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let name = arg
            .strip_prefix("--")
            .filter(|n| allowed.split(' ').any(|a| a == *n))
            .ok_or_else(|| format!("unknown argument {arg}\n{USAGE}"))?;
        let value = it
            .next()
            .filter(|v| !v.starts_with("--"))
            .ok_or_else(|| format!("{arg} needs a value"))?;
        if out.insert(name.to_string(), value.clone()).is_some() {
            return Err(format!("{arg} given twice"));
        }
    }
    Ok(out)
}

fn parsed<T: std::str::FromStr>(
    f: &HashMap<String, String>,
    name: &str,
    default: Option<T>,
) -> Result<T, String> {
    match f.get(name) {
        Some(v) => v
            .parse()
            .map_err(|_| format!("--{name}: invalid value {v}")),
        None => default.ok_or_else(|| format!("--{name} is required")),
    }
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn load_manifest(path: &Path) -> Result<Vec<Task>, String> {
    let text = read(path)?;
    let doc: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let tasks = doc["tasks"].as_array().ok_or("manifest: no tasks array")?;
    if tasks.is_empty() {
        return Err("manifest: empty tasks array".to_string());
    }
    let mut ids = std::collections::HashSet::new();
    tasks
        .iter()
        .map(|t| {
            let s = |k: &str| t[k].as_str().map(str::to_string);
            let id = s("id").ok_or("task without id")?;
            let err = |m: &str| format!("task {id}: {m}");
            if !ids.insert(id.clone()) {
                return Err(err("duplicate id"));
            }
            let strings = |k: &str| {
                serde_json::from_value::<Vec<String>>(t[k].clone())
                    .map_err(|e| err(&format!("{k}: {e}")))
            };
            let answers = strings("answers")?;
            if answers.is_empty() || answers.iter().any(|a| a.trim().is_empty()) {
                return Err(err("answers must be non-empty"));
            }
            let arms = strings("arms")?;
            if let Some(a) = arms.iter().find(|a| !ARMS.contains(&a.as_str())) {
                return Err(err(&format!("unknown arm {a}")));
            }
            if let Some(a) =
                (1..arms.len()).find_map(|i| arms[..i].contains(&arms[i]).then_some(&arms[i]))
            {
                return Err(err(&format!("duplicate arm {a}")));
            }
            let evidence = s("evidence");
            let control = s("control");
            if let Some(c) = control.as_deref()
                && !matches!(c, "incompressible" | "evidence_drop")
            {
                return Err(err(&format!("unknown control {c:?}")));
            }
            if arms.iter().any(|a| a == "drop-evidence") {
                if control.as_deref() != Some("evidence_drop") {
                    return Err(err("drop-evidence needs control evidence_drop"));
                }
                if evidence.as_deref().is_none_or(str::is_empty) {
                    return Err(err("drop-evidence needs a non-empty evidence string"));
                }
            }
            let tolerance = match &t["tolerance"] {
                Value::Null => 0.0,
                v => v
                    .as_f64()
                    .filter(|x| *x >= 0.0)
                    .ok_or_else(|| err("tolerance must be a number >= 0"))?,
            };
            // ponytail: the manifest is trusted and committed, so an input may be absolute or leave its directory.
            let input_path = dir.join(s("input").ok_or_else(|| err("no input"))?);
            let rule = s("rule")
                .as_deref()
                .and_then(Rule::parse)
                .ok_or_else(|| err("unknown rule"))?;
            let finite = |a: &String| score::normalize(a).parse().is_ok_and(f64::is_finite);
            if rule == Rule::Number && !answers.iter().all(finite) {
                return Err(err("number answers must parse as finite numbers"));
            }
            Ok(Task {
                input: read(&input_path).map_err(|e| err(&e))?,
                category: s("category").ok_or_else(|| err("no category"))?,
                control,
                question: s("question").ok_or_else(|| err("no question"))?,
                rule,
                tolerance,
                answers,
                arms,
                evidence,
                id,
            })
        })
        .collect()
}

/// The arm's payload. `None` when the transform declines, so the payload is
/// the original.
fn variant(arm: &str, task: &Task) -> Option<String> {
    let input = task.input.as_str();
    let only = |json_table, exact_line_runs, line_templates| CompressionContext {
        json_minify: false,
        json_table,
        exact_line_runs,
        line_templates,
        token_counter: Some(&EstimateCounter),
        ..FIRST_SLICE_JSON_MINIFY_CONTEXT
    };
    let out = match arm {
        "json-minify" => minify_json(input),
        "table-csv" => compress(input, &only(true, false, false)).into_changed(),
        // Same row and depth limits as the table-csv arm, so both formats see the same inputs.
        "table-mdkv" => table::render_markdown_kv(
            input,
            &TableOptions {
                max_rows: FIRST_SLICE_JSON_MINIFY_CONTEXT.limits.max_table_rows,
                max_depth: FIRST_SLICE_JSON_MINIFY_CONTEXT.limits.max_json_depth,
                token_counter: Some(&EstimateCounter),
                ..TableOptions::default()
            },
        ),
        "lines-v1" => compress(input, &only(false, true, false)).into_changed(),
        "lines-v2" => compress(input, &only(false, false, true)).into_changed(),
        "drop-evidence" => task.evidence.as_deref().map(|ev| {
            input
                .split_inclusive('\n')
                .filter(|l| !l.contains(ev))
                .collect()
        }),
        _ => None,
    };
    out.filter(|o| o != input)
}

fn tokens(text: &str) -> u64 {
    EstimateCounter.count(text).count
}

fn user_prompt(payload: &str, question: &str) -> String {
    format!("<tool_result>\n{payload}\n</tool_result>\n\nQuestion: {question}")
}

/// Every (task, arm) pair in manifest order, baseline first per task.
fn pairs(tasks: &[Task]) -> Vec<(&Task, &str)> {
    tasks
        .iter()
        .flat_map(|t| {
            std::iter::once("baseline")
                .chain(t.arms.iter().map(String::as_str))
                .map(move |a| (t, a))
        })
        .collect()
}

fn variants(manifest: &Path) -> Result<(), String> {
    let tasks = load_manifest(manifest)?;
    println!(
        "| Task | Arm | Bytes | Estimated tokens | Changed |\n| --- | --- | --- | --- | --- |"
    );
    for (task, arm) in pairs(&tasks) {
        let v = variant(arm, task);
        let payload = v.as_deref().unwrap_or(&task.input);
        println!(
            "| {} | {arm} | {} | {} | {} |",
            task.id,
            payload.len(),
            tokens(payload),
            if v.is_some() { "yes" } else { "no" }
        );
    }
    Ok(())
}

/// Offline evidence-presence check: answers with the first accepted answer
/// when it (every item, for `set`) occurs verbatim in the payload.
fn ask_mock(task: &Task, payload: &str) -> Value {
    let first = &task.answers[0];
    let present = match task.rule {
        Rule::Set => first.split(',').map(str::trim).all(|i| payload.contains(i)),
        _ => payload.contains(first.as_str()),
    };
    json!({ "answer": if present { first.as_str() } else { "NOT_FOUND" }, "backend_error": false })
}

/// One `claude -p` call in `dir`, which holds only a project settings file.
fn ask_claude(dir: &Path, model: &str, per_run: f64, prompt: &str) -> Result<Value, String> {
    let mut child = Command::new("claude")
        .args("-p --output-format json --setting-sources project --strict-mcp-config".split(' '))
        .args(["--no-session-persistence", "--tools", "", "--model", model])
        .args(["--max-budget-usd", &per_run.to_string()])
        .args(["--system-prompt", SYSTEM_PROMPT])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start claude: {e}"))?;
    // ponytail: the client reads all of stdin before it writes, so no writer thread.
    let written = child
        .stdin
        .take()
        .map(|mut s| s.write_all(prompt.as_bytes()));
    let out = child
        .wait_with_output()
        .map_err(|e| format!("claude: {e}"))?;
    let v: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    let usage = &v["usage"];
    let ok = out.status.success() && matches!(written, Some(Ok(()))) && v["is_error"] == false;
    Ok(json!({
        "answer": v["result"].as_str().unwrap_or(""),
        "backend_error": !ok,
        "input_tokens": usage["input_tokens"],
        "output_tokens": usage["output_tokens"],
        "cache_read_tokens": usage["cache_read_input_tokens"],
        "cache_creation_tokens": usage["cache_creation_input_tokens"],
        "cost_usd": v["total_cost_usd"],
        "duration_ms": v["duration_ms"],
        "turns": v["num_turns"],
    }))
}

/// A throwaway project directory whose settings disable every hook.
fn live_dir() -> Result<PathBuf, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let parent = std::env::temp_dir();
    fs::create_dir_all(&parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let dir = parent.join(format!("deslag-eval-{}-{nanos}", std::process::id()));
    let settings = dir.join(".claude");
    // `create` refuses an existing path, so no one else can own the settings directory.
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    let made = builder
        .create(&settings)
        .and_then(|()| {
            fs::write(
                settings.join("settings.json"),
                "{\"disableAllHooks\": true}\n",
            )
        })
        .map_err(|e| format!("{}: {e}", settings.display()));
    // `run` registers the directory for cleanup only on success, so a failed setup removes it here.
    if made.is_err() {
        let _ = fs::remove_dir_all(&dir);
    }
    made.map(|()| dir)
}

fn run(args: &[String]) -> Result<(), String> {
    let f = flags(
        args,
        "manifest backend seed out model budget-usd per-run-usd",
    )?;
    let backend: String = parsed(&f, "backend", None)?;
    let live = match backend.as_str() {
        "mock" => false,
        "claude" => true,
        other => return Err(format!("unknown backend {other}")),
    };
    // Checked before anything else, so no setting can reach a paid call by accident.
    if live && std::env::var("DESLAG_EVAL_LIVE").as_deref() != Ok("1") {
        return Err(
            "the claude backend calls a paid model; set DESLAG_EVAL_LIVE=1 to allow it".into(),
        );
    }
    let model: Option<String> = f.get("model").filter(|m| !m.trim().is_empty()).cloned();
    let budget: f64 = parsed(&f, "budget-usd", (!live).then_some(0.0))?;
    let per_run: f64 = parsed(&f, "per-run-usd", Some(DEFAULT_PER_RUN_USD))?;
    if live && (model.is_none() || !usd_valid(per_run, &[budget])) {
        return Err(
            "the claude backend needs --model, --budget-usd >= 0 and --per-run-usd > 0".into(),
        );
    }
    let seed: u64 = parsed(&f, "seed", None)?;
    let out_path: String = parsed(&f, "out", None)?;
    let tasks = load_manifest(Path::new(&parsed::<String>(&f, "manifest", None)?))?;

    let client = if live {
        let v = Command::new("claude")
            .arg("--version")
            .output()
            .map_err(|e| format!("cannot start claude: {e}"))?;
        String::from_utf8_lossy(&v.stdout).trim().to_string()
    } else {
        format!("deslag-eval {}", deslag_core::VERSION)
    };
    // Opened before the live directory exists, so a refused path leaks nothing.
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&out_path)
        .map_err(|e| match e.kind() {
            io::ErrorKind::AlreadyExists => format!("{out_path} exists; pass a new --out path"),
            _ => format!("{out_path}: {e}"),
        })?;
    let dir = if live { Some(live_dir()?) } else { None };
    let mut order = pairs(&tasks);
    score::shuffle(&mut order, seed);
    let mut spent = 0.0;
    let result = (|| {
        for (index, (task, arm)) in order.into_iter().enumerate() {
            if live && spent + per_run > budget {
                return Err(format!(
                    "budget stop before run {index}: spent {spent:.4} of {budget} USD"
                ));
            }
            let v = variant(arm, task);
            let payload = v.as_deref().unwrap_or(&task.input);
            let prompt = user_prompt(payload, &task.question);
            let started = Instant::now();
            let mut reply = match &dir {
                Some(d) => ask_claude(d, model.as_deref().unwrap_or(""), per_run, &prompt)?,
                None => ask_mock(task, payload),
            };
            // The mock records 0, so its report depends only on the manifest and the seed.
            let wall_ms = if live {
                started.elapsed().as_millis() as u64
            } else {
                0
            };
            reply["wall_ms"] = json!(wall_ms);
            // A negative or non-finite reported cost counts as the per-run cap, like a missing one.
            let cost = reply["cost_usd"]
                .as_f64()
                .filter(|c| c.is_finite() && *c >= 0.0);
            if live {
                spent += cost.unwrap_or(per_run);
            }
            let answer = reply["answer"].as_str().unwrap_or("");
            let kind = if reply["backend_error"] == true {
                ErrorKind::Backend
            } else {
                score::score(answer, &task.answers, task.rule, task.tolerance)
            };
            let mut line = json!({
                "task": task.id, "category": task.category, "control": task.control, "arm": arm,
                "seed": seed, "order": index, "backend": backend, "model": model, "client_version": client,
                "variant_unchanged": v.is_none(), "payload_bytes": payload.len(), "payload_tokens_est": tokens(payload),
                "success": kind == ErrorKind::None, "error_kind": kind.as_str(),
            });
            if let (Value::Object(l), Value::Object(r)) = (&mut line, reply) {
                l.extend(r);
            }
            writeln!(out, "{line}").map_err(|e| format!("{out_path}: {e}"))?;
            // The client checks its cap only after a turn, so one call can exceed it.
            if let Some(c) = cost.filter(|c| live && *c > per_run) {
                return Err(format!(
                    "cost stop after run {index}: it reported {c:.4} USD, above --per-run-usd {per_run}"
                ));
            }
        }
        Ok(())
    })();
    if let Some(d) = dir {
        let _ = fs::remove_dir_all(d);
    }
    eprintln!("deslag-eval: spent {spent:.4} USD");
    result
}

/// True when `per_run` is finite and above 0 and every one of `others` is finite and >= 0.
fn usd_valid(per_run: f64, others: &[f64]) -> bool {
    per_run.is_finite() && per_run > 0.0 && others.iter().all(|x| x.is_finite() && *x >= 0.0)
}

/// Estimated cost of a full live run under stated assumptions (estimated
/// prompt tokens, assumed prices and output tokens), plus runs times
/// `--per-run-usd`, a cap only if every call stays within `--per-run-usd`.
fn estimate(args: &[String]) -> Result<(), String> {
    let f = flags(
        args,
        "manifest input-usd-per-mtok output-usd-per-mtok overhead-tokens output-tokens per-run-usd",
    )?;
    let tasks = load_manifest(Path::new(&parsed::<String>(&f, "manifest", None)?))?;
    let in_price: f64 = parsed(&f, "input-usd-per-mtok", Some(3.0))?;
    let out_price: f64 = parsed(&f, "output-usd-per-mtok", Some(15.0))?;
    let overhead: u64 = parsed(&f, "overhead-tokens", Some(2000))?;
    let out_tokens: u64 = parsed(&f, "output-tokens", Some(256))?;
    let per_run: f64 = parsed(&f, "per-run-usd", Some(DEFAULT_PER_RUN_USD))?;
    if !usd_valid(per_run, &[in_price, out_price]) {
        return Err("estimate needs prices per Mtok >= 0 and --per-run-usd > 0".into());
    }
    let runs = pairs(&tasks);
    let input: u64 = runs
        .iter()
        .map(|(t, a)| {
            let payload = variant(a, t).unwrap_or_else(|| t.input.clone());
            tokens(SYSTEM_PROMPT) + tokens(&user_prompt(&payload, &t.question)) + overhead
        })
        .sum();
    let n = runs.len() as u64;
    // Every input token is priced as a cache write (1.25x), the costliest input kind.
    let cost = (input as f64 * in_price * 1.25 + (n * out_tokens) as f64 * out_price) / 1e6;
    let cap = n as f64 * per_run;
    println!(
        "| Field | Value |
| --- | --- |
| Runs (task, arm) | {n} |
| Estimated input tokens (deslag-estimate-v1, incl. {overhead} overhead per run) | {input} |
| Assumed output tokens per run | {out_tokens} |
| Assumed prices, USD per Mtok | input {in_price} (x1.25 cache write), output {out_price} |
| Estimated cost | USD {cost:.2} |
| Cap if every call stays within --per-run-usd ({n} runs at {per_run}) | USD {cap:.2} |"
    );
    Ok(())
}
