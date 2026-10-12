//! Deterministic corpus generator. The same seed yields the same files byte
//! for byte. Every answer is built from the generated rows, then recomputed
//! from the written payload text by an independent solver; a task whose
//! answer differs, is not unique or is not verbatim in the payload is refused.
//! Payloads are synthetic: documentation IP ranges and made-up names.

use std::{fs, path::Path};

use serde_json::{Value, json};

use crate::score::{shuffle, splitmix64};

const JSON_ARMS: [&str; 4] = ["json-minify", "table-csv", "table-mdkv", "table-v2"];
const LINE_ARMS: [&str; 2] = ["lines-v1", "lines-v2"];
const ID_CHARS: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const HEX: &[u8] = b"0123456789abcdef";
const ADJ: [&str; 12] = [
    "amber", "brisk", "calm", "dusty", "eager", "fuzzy", "gentle", "hazy", "ivory", "jolly",
    "keen", "lunar",
];
const NOUN: [&str; 12] = [
    "badger", "cedar", "dingo", "egret", "falcon", "gecko", "heron", "iguana", "jackal", "koala",
    "lemur", "marmot",
];
const TEAMS: [&str; 6] = [
    "alpha-team",
    "beta-team",
    "core-team",
    "data-team",
    "edge-team",
    "infra-team",
];
const MODS: [&str; 6] = [
    "billing::invoice",
    "billing::ledger",
    "auth::session",
    "auth::token",
    "search::index",
    "search::query",
];
const NOISE: [&str; 4] = [
    "waiting for upstream to accept connections",
    "retrying after a transient error",
    "heartbeat ok",
    "flushing buffered writes",
];
const FIRST: [&str; 6] = ["Oskar", "Mina", "Tavi", "Reka", "Joran", "Selma"];
const LAST: [&str; 6] = ["Lindqvist", "Okafor", "Brandt", "Varga", "Haddad", "Moreau"];

/// How to pick the answer out of the matching rows of a JSON payload.
pub enum Pick {
    /// The field of the one matching row.
    Field(&'static str),
    /// The field of every matching row (at least two), sorted, comma-joined.
    Set(&'static str),
    /// The matching row with the unique highest `num`: its `name` field, or
    /// the number itself when `name` is `None`.
    Max {
        num: &'static str,
        name: Option<&'static str>,
    },
}

/// The question as a structured query over the written payload text.
pub enum Query {
    /// Rows of a root array (or the root object) that equal every condition.
    Json {
        conds: Vec<(&'static str, Value)>,
        pick: Pick,
    },
    /// The one line containing `contains`; the answer is the text after
    /// `after` up to the first character in `end`, or to the line end.
    Scan {
        contains: String,
        after: String,
        end: &'static str,
    },
}

fn solve(input: &str, q: &Query) -> Result<String, String> {
    match q {
        Query::Scan {
            contains,
            after,
            end,
        } => {
            let mut hits = input.lines().filter(|l| l.contains(contains.as_str()));
            let (Some(line), None) = (hits.next(), hits.next()) else {
                return Err(format!("{contains:?} must match exactly one line"));
            };
            let at = line.find(after.as_str()).ok_or("after prefix missing")? + after.len();
            let rest = &line[at..];
            Ok(rest[..rest.find(|c| end.contains(c)).unwrap_or(rest.len())].to_string())
        }
        Query::Json { conds, pick } => {
            let doc: Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
            let rows = match &doc {
                Value::Array(a) => a.iter().collect(),
                o => vec![o],
            };
            let hits: Vec<&Value> = rows
                .into_iter()
                .filter(|r| conds.iter().all(|(k, v)| r.get(*k) == Some(v)))
                .collect();
            let text = |r: &Value, k: &str| match &r[k] {
                Value::String(s) => Ok(s.clone()),
                Value::Null => Err(format!("{k} is null or missing")),
                v => Ok(v.to_string()),
            };
            match pick {
                Pick::Field(k) => match hits[..] {
                    [r] => text(r, k),
                    _ => Err(format!("{} rows match, need 1", hits.len())),
                },
                Pick::Set(k) if hits.len() >= 2 => {
                    let mut v = hits
                        .iter()
                        .map(|r| text(r, k))
                        .collect::<Result<Vec<_>, _>>()?;
                    v.sort();
                    Ok(v.join(","))
                }
                Pick::Set(_) => Err(format!("{} rows match, need 2 or more", hits.len())),
                Pick::Max { num, name } => {
                    let val = |r: &&Value| r[*num].as_f64();
                    let top = hits
                        .iter()
                        .filter_map(val)
                        .fold(f64::NEG_INFINITY, f64::max);
                    let best: Vec<_> = hits.iter().filter(|r| val(r) == Some(top)).collect();
                    match (best.len(), name) {
                        (1, Some(k)) => text(best[0], k),
                        (1, None) => Ok(top.to_string()),
                        (n, _) => Err(format!("{n} rows tie for the maximum")),
                    }
                }
            }
        }
    }
}

/// Recomputes the answer from the payload text and refuses a mismatch, a
/// non-unique answer, or an answer item that is not verbatim in the payload.
pub fn check(input: &str, q: &Query, answer: &str) -> Result<(), String> {
    let got = solve(input, q)?;
    // A number answer keeps its lexeme in the payload; the solver sees its value.
    let number = matches!(
        q,
        Query::Json {
            pick: Pick::Max { name: None, .. },
            ..
        }
    );
    let same = if number {
        got.parse::<f64>().ok() == answer.parse::<f64>().ok()
    } else {
        got == answer
    };
    if !same {
        return Err(format!("recomputed {got:?}, expected {answer:?}"));
    }
    // A number answer must be a whole value token: after ':' and before ',', '}' or a line end.
    if number
        && !input.match_indices(answer).any(|(i, _)| {
            input[..i].trim_end().ends_with(':')
                && matches!(
                    input[i + answer.len()..].chars().next(),
                    Some(',' | '}' | '\n' | '\r')
                )
        })
    {
        return Err(format!(
            "number {answer:?} is not a whole value in the payload"
        ));
    }
    answer
        .split(',')
        .find(|i| !input.contains(*i))
        .map_or(Ok(()), |i| {
            Err(format!("answer item {i:?} is not in the payload"))
        })
}

/// Refuses a buried table whose answer row sits above 60% depth:
/// every row that matches the conditions needs index * 10 >= rows * 6.
pub fn check_buried(input: &str, q: &Query) -> Result<(), String> {
    let Query::Json { conds, .. } = q else {
        return Err("a buried table needs a JSON query".into());
    };
    let doc: Value = serde_json::from_str(input).map_err(|e| e.to_string())?;
    let rows = doc.as_array().ok_or("a buried table needs a root array")?;
    for (at, r) in rows.iter().enumerate() {
        if conds.iter().all(|(k, v)| r.get(*k) == Some(v)) && at * 10 < rows.len() * 6 {
            return Err(format!(
                "answer row {at} of {} is above 60% depth",
                rows.len()
            ));
        }
    }
    Ok(())
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        splitmix64(&mut self.0)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn code(&mut self, len: usize, alphabet: &[u8]) -> String {
        (0..len)
            .map(|_| alphabet[self.below(alphabet.len())] as char)
            .collect()
    }

    /// `n` distinct adjective-noun names (n <= 144).
    fn names(&mut self, n: usize) -> Vec<String> {
        let mut all: Vec<String> = ADJ
            .iter()
            .flat_map(|a| NOUN.iter().map(move |b| format!("{a}-{b}")))
            .collect();
        shuffle(&mut all, self.next());
        all.truncate(n);
        all
    }
}

/// `s` with its last character replaced, so it differs in one place only.
/// An IP octet of at most 249 stays at most 248, so the result stays valid.
fn near(s: &str) -> String {
    let (head, last) = s.split_at(s.len() - 1);
    format!("{head}{}", if last == "7" { '8' } else { '7' })
}

struct Gen {
    id: String,
    category: &'static str,
    control: Option<&'static str>,
    ext: &'static str,
    input: String,
    question: String,
    answer: String,
    rule: &'static str,
    arms: Vec<&'static str>,
    evidence: Option<String>,
    query: Query,
}

type Qa = (String, String, Query);

fn task(
    id: String,
    category: &'static str,
    (ext, rule, arms): (&'static str, &'static str, &[&'static str]),
    input: String,
    (question, answer, query): Qa,
) -> Gen {
    Gen {
        id,
        category,
        control: None,
        ext,
        input,
        question,
        answer,
        rule,
        arms: arms.to_vec(),
        evidence: None,
        query,
    }
}

fn json_query(conds: Vec<(&'static str, Value)>, pick: Pick) -> Query {
    Query::Json { conds, pick }
}

fn scan(contains: &str, after: &str, end: &'static str) -> Query {
    Query::Scan {
        contains: contains.into(),
        after: after.into(),
        end,
    }
}

fn max_of(
    conds: Vec<(&'static str, Value)>,
    num: &'static str,
    name: Option<&'static str>,
) -> Query {
    json_query(conds, Pick::Max { num, name })
}

fn quoted(s: &str) -> String {
    format!("\"{s}\"")
}

/// Pretty-prints objects with 2-space indent over raw lexemes: key order is
/// the given order and number lexemes survive.
fn pretty(rows: &[Vec<(&str, String)>]) -> String {
    let object = |fields: &Vec<(&str, String)>| {
        let lines: Vec<_> = fields
            .iter()
            .map(|(k, v)| format!("    \"{k}\": {v}"))
            .collect();
        format!("  {{\n{}\n  }}", lines.join(",\n"))
    };
    let objects: Vec<String> = rows.iter().map(object).collect();
    format!("[\n{}\n]\n", objects.join(",\n"))
}

struct Schema {
    category: &'static str,
    noun: &'static str,
    plural: &'static str,
    /// id, name, status, num, note
    keys: [&'static str; 5],
    /// Whether `num` has a fractional meaning, so decimal lexemes fit.
    frac: bool,
    /// The status of most rows, of the first special row, and of the three rows form 3 lists.
    status: [&'static str; 3],
    prefix: &'static str,
}

#[rustfmt::skip]
const SCHEMAS: [Schema; 7] = [
    Schema { category: "api_query", noun: "user", plural: "users", keys: ["user_id", "login", "role", "quota_gb", "team"], frac: true, status: ["member", "owner", "auditor"], prefix: "usr" },
    Schema { category: "code_search", noun: "match", plural: "matches", keys: ["match_id", "symbol", "kind", "line", "crate"], frac: false, status: ["fn", "macro", "const"], prefix: "mat" },
    Schema { category: "failure_diagnosis", noun: "CI job", plural: "CI jobs", keys: ["job_id", "name", "conclusion", "duration_s", "runner"], frac: true, status: ["success", "failure", "cancelled"], prefix: "job" },
    Schema { category: "issue_triage", noun: "issue", plural: "issues", keys: ["issue_id", "title", "state", "comments", "assignee"], frac: false, status: ["closed", "blocked", "stale"], prefix: "iss" },
    Schema { category: "aggregation", noun: "service", plural: "services", keys: ["service_id", "service_name", "tier", "error_rate", "owner"], frac: true, status: ["bronze", "gold", "silver"], prefix: "svc" },
    Schema { category: "exact_id", noun: "order", plural: "orders", keys: ["order_id", "customer", "status", "amount_cents", "coupon"], frac: false, status: ["paid", "refunded", "disputed"], prefix: "ord" },
    Schema { category: "buried_evidence", noun: "audit event", plural: "audit events", keys: ["event_id", "target", "action", "seq", "actor"], frac: false, status: ["read", "delete", "rotate"], prefix: "evt" },
];

struct Row {
    id: String,
    name: String,
    status: String,
    num: String,
    /// A lexeme, or `None` when the field is missing.
    note: Option<String>,
}

/// A number lexeme that grows with `i`, so lexemes are distinct. With
/// `zero`, a decimal one ends in 0 (like 1204.10) and is kept verbatim.
fn num(rng: &mut Rng, decimal: bool, i: usize, zero: bool) -> String {
    let base = 100 + i * 7 + rng.below(7);
    match (decimal, zero) {
        (false, _) => base.to_string(),
        (true, true) => format!("{base}.{:02}", (1 + rng.below(9)) * 10),
        (true, false) => format!("{base}.{:02}", rng.below(100)),
    }
}

fn json_task(t: usize, rng: &mut Rng) -> Gen {
    // Only every third audit slot is a buried table; the others reuse another schema.
    let si = if t % 7 == 6 && !(t / 7).is_multiple_of(3) {
        t / 7 % 6
    } else {
        t % 7
    };
    let s = &SCHEMAS[si];
    // A buried table only gets lookup questions: forms 0 to 2.
    let buried = si == 6;
    let form = rng.below(if buried { 3 } else { 7 });
    let (base, spread) = if buried { (100, 3) } else { (10, 5) };
    let n = base + rng.below(spread);
    let (decimal, ragged) = (s.frac, form == 6 || rng.below(3) == 0);
    let names = rng.names(n);
    let mut rows = Vec::new();
    for (i, name) in names.into_iter().enumerate() {
        let team = quoted(TEAMS[rng.below(TEAMS.len())]);
        // A ragged table has missing and null notes next to present ones; others have no note column.
        let note = match (ragged, rng.below(6)) {
            (true, 1 | 2) => Some("null".to_string()),
            (true, 3..) => Some(team),
            _ => None,
        };
        rows.push(Row {
            id: format!("{}_{}", s.prefix, rng.code(8, ID_CHARS)),
            name,
            status: s.status[0].into(),
            num: num(rng, decimal, i, false),
            note,
        });
    }
    let take = [2, 2, 2, 4, 2, 2, 3][form];
    let mut special: Vec<Row> = rows.drain(..take).collect();
    // The last special row is a near-duplicate decoy of the first one.
    let decoy = special.len() - 1;
    special[decoy].id = near(&special[0].id);
    special[decoy].name = format!("{}-2", special[0].name);
    let top = |rng: &mut Rng, k: usize| num(rng, decimal, n + k, true);
    special[0].status = s.status[1].into();
    let [id, name, status, numk, note] = s.keys;
    let (t0, noun, plural) = (&special[0], s.noun, s.plural);
    let string = |x: &str| Value::String(x.into());
    let qa: Qa = match form {
        0 => (
            format!(
                "What is the {id} of the {noun} whose {name} is {}?",
                t0.name
            ),
            t0.id.clone(),
            json_query(vec![(name, string(&t0.name))], Pick::Field(id)),
        ),
        1 => (
            format!("What is the {name} of the {noun} with {id} {}?", t0.id),
            t0.name.clone(),
            json_query(vec![(id, string(&t0.id))], Pick::Field(name)),
        ),
        2 => {
            let lexeme = top(rng, 3);
            special[0].num = lexeme.clone();
            (
                format!("What is the {id} of the {noun} with {numk} {lexeme}?"),
                special[0].id.clone(),
                json_query(
                    vec![(numk, serde_json::from_str(&lexeme).unwrap())],
                    Pick::Field(id),
                ),
            )
        }
        3 => {
            for r in &mut special[..3] {
                r.status = s.status[2].into();
            }
            let mut ids: Vec<&str> = special[..3].iter().map(|r| r.id.as_str()).collect();
            ids.sort_unstable();
            (
                format!(
                    "List the {id} values of all {plural} with {status} {}, comma-separated.",
                    s.status[2]
                ),
                ids.join(","),
                json_query(vec![(status, string(s.status[2]))], Pick::Set(id)),
            )
        }
        4 => {
            special[0].num = top(rng, 3);
            (
                format!("Which {noun} has the highest {numk} value? Answer with its {name}."),
                special[0].name.clone(),
                max_of(vec![], numk, Some(name)),
            )
        }
        6 => {
            // Null and absent notes sit side by side; only an explicit null counts.
            for r in &mut special {
                r.note = Some("null".into());
            }
            let nulls = rows
                .iter()
                .chain(&special)
                .filter(|r| r.note.as_deref() == Some("null"));
            let mut ids: Vec<&str> = nulls.map(|r| r.id.as_str()).collect();
            ids.sort_unstable();
            (
                format!(
                    "List the {id} values of all {plural} whose {note} is null, comma-separated. A row with no {note} field does not count.",
                ),
                ids.join(","),
                json_query(vec![(note, Value::Null)], Pick::Set(id)),
            )
        }
        _ => {
            // Row 0 holds the global maximum under another status; row 1 the answer.
            special[0].num = top(rng, 5);
            special[1].status = s.status[0].into();
            special[1].num = top(rng, 3);
            (
                format!(
                    "What is the highest {numk} value among {plural} with {status} {}?",
                    s.status[0]
                ),
                special[1].num.clone(),
                max_of(vec![(status, string(s.status[0]))], numk, None),
            )
        }
    };
    shuffle(&mut rows, rng.next());
    // The answer rows of a buried task sit in the last 40% of the table.
    let lo = if buried { (n * 6).div_ceil(10) } else { 0 };
    for r in special {
        let at = lo + rng.below(rows.len() - lo + 1);
        rows.insert(at, r);
    }
    let objects: Vec<_> = rows
        .iter()
        .map(|r| {
            let mut f = vec![
                (id, quoted(&r.id)),
                (name, quoted(&r.name)),
                (status, quoted(&r.status)),
                (numk, r.num.clone()),
            ];
            f.extend(r.note.clone().map(|v| (note, v)));
            f
        })
        .collect();
    let rule = match form {
        3 | 6 => "set",
        5 => "number",
        _ => "exact",
    };
    task(
        format!("gen-json-{t:03}"),
        s.category,
        ("json", rule, &JSON_ARMS),
        pretty(&objects),
        qa,
    )
}

fn log_task(t: usize, rng: &mut Rng) -> Gen {
    let kind = [0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2, 3][t % 16];
    let buried = kind == 3;
    let (base, spread) = if buried { (100, 5) } else { (12, 11) };
    let n = base + rng.below(spread);
    let lo = if buried { (n * 6).div_ceil(10) } else { 0 };
    let p = lo + rng.below(n - lo);
    let d = (p + 1 + rng.below(n - 1)) % n;
    let names = rng.names(n + 2);
    let a = match kind {
        0 => format!("req-{}", rng.code(6, HEX)),
        1 => format!("203.0.113.{}", 1 + rng.below(249)),
        2 => format!("{}::{}", MODS[rng.below(6)], names[n].replace('-', "_")),
        _ => rng.code(64, HEX),
    };
    let dc = near(&a);
    let art = format!("deslag-{}", names[n + 1]);
    let hms = |i: usize| {
        let sec = i * 7;
        let (h, m, s) = (10 + sec / 3600, sec / 60 % 60, sec % 60);
        format!("{h:02}:{m:02}:{s:02}")
    };
    let mut lines = Vec::new();
    for i in 0..n {
        let (hit, decoy) = (i == p, i == d);
        lines.push(match kind {
            0 => {
                let (id, code) = match (hit, decoy) {
                    (true, _) => (a.clone(), 503),
                    (_, true) => (dc.clone(), 200),
                    _ => (format!("req-{}", rng.code(6, HEX)), [200, 200, 201, 404, 500][rng.below(5)]),
                };
                format!("2026-09-30T{}Z INFO request_id={id} status={code} upstream={}", hms(i), TEAMS[i % 6])
            }
            1 => {
                let (ip, code) = match (hit, decoy) {
                    (true, _) => (a.clone(), 403),
                    (_, true) => (dc.clone(), 200),
                    _ => (format!("198.51.100.{}", 1 + rng.below(250)), [200, 200, 304, 404][rng.below(4)]),
                };
                let bytes = 100 + rng.below(900);
                format!("{ip} - - [30/Sep/2026:{} +0000] \"GET /api/v2/items/{} HTTP/1.1\" {code} {bytes}", hms(i), i * 37)
            }
            2 => {
                let (path, verdict) = match (hit, decoy) {
                    (true, _) => (a.clone(), "FAILED"),
                    (_, true) => (dc.clone(), "ok"),
                    _ => (format!("{}::{}", MODS[i % 6], names[i].replace('-', "_")), "ok"),
                };
                format!("test {path} ... {verdict}")
            }
            _ if hit => format!("artifact {art}.tar.gz sha256={a}"),
            _ if decoy => format!("artifact {art}-debug.tar.gz sha256={dc}"),
            _ => format!("[{:>3}/{n}] Compiling crate_{i:03} v{}.{}.{}", i + 1, i % 9, i % 7, i % 5),
        });
    }
    if kind == 2 {
        lines.extend([
            "failures:".into(),
            format!("    {a}"),
            format!("test result: FAILED. {} passed; 1 failed", n - 1),
        ]);
    }
    // Runs of identical lines before the answer line never touch it.
    for _ in 0..2 {
        let (msg, run, at) = (NOISE[rng.below(4)], 4 + rng.below(5), rng.below(p + 1));
        lines.splice(at..at, vec![msg.to_string(); run]);
    }
    let input = lines.join("\n") + "\n";
    let (category, question, query) = match kind {
        0 => (
            "logs",
            "Which request_id failed with status 503?".to_string(),
            scan("status=503 ", "request_id=", " "),
        ),
        1 => (
            "logs",
            "Which client IP address received the only 403 response?".into(),
            scan("\" 403 ", "", " "),
        ),
        2 => (
            "failure_diagnosis",
            "Which test failed? Answer with the full test path.".into(),
            scan(" ... FAILED", "test ", " "),
        ),
        _ => (
            "buried_evidence",
            format!("What sha256 checksum is reported for the artifact {art}.tar.gz?"),
            scan(&format!("artifact {art}.tar.gz "), "sha256=", " "),
        ),
    };
    let rule = if buried { "exact_ci" } else { "exact" };
    task(
        format!("gen-log-{t:03}"),
        category,
        ("log", rule, &LINE_ARMS),
        input,
        (question, a, query),
    )
}

/// Incompressible controls: one prose line, or an already minimal JSON object.
fn incompressible(k: usize, rng: &mut Rng) -> Gen {
    let id = format!("gen-ctl-incompressible-{k:02}");
    let g = if k.is_multiple_of(2) {
        let date = format!("2026-{:02}-{:02}", 10 + rng.below(3), 1 + rng.below(28));
        let who = format!("{} {}", FIRST[rng.below(6)], LAST[rng.below(6)]);
        let lead = format!("on-call engineer for the week of {date} is ");
        let input = format!(
            "Rotation notes for the {}. The {lead}{who}. Escalations go to the secondary after {} minutes without acknowledgement. The handover call moved to {} because of a public holiday.\n",
            TEAMS[rng.below(6)],
            5 * (1 + rng.below(6)),
            ["Monday", "Tuesday", "Thursday"][rng.below(3)],
        );
        let query = scan(&lead, &lead, ".");
        task(
            id,
            "incompressible",
            ("txt", "exact_ci", &[&JSON_ARMS[..], &LINE_ARMS].concat()),
            input,
            (
                format!("Who is the on-call engineer for the week of {date}?"),
                who,
                query,
            ),
        )
    } else {
        let (job, rows) = (rng.names(1).remove(0), 1000 + rng.below(90000));
        let input = format!("{{\"job\":\"{job}\",\"state\":\"succeeded\",\"rows\":{rows}}}");
        let query = json_query(vec![], Pick::Field("rows"));
        task(
            id,
            "incompressible",
            ("json", "number", &[&JSON_ARMS[..], &LINE_ARMS].concat()),
            input,
            (
                format!("How many rows did the {job} job write?"),
                rows.to_string(),
                query,
            ),
        )
    };
    Gen {
        control: Some("incompressible"),
        ..g
    }
}

/// Evidence-drop controls: one answer line holds the evidence string, which
/// `drop-evidence` removes and every transform keeps.
fn evidence_drop(k: usize, rng: &mut Rng) -> Gen {
    let id = format!("gen-ctl-evidence-drop-{k:02}");
    let n = 20 + rng.below(12);
    let g = if k.is_multiple_of(2) {
        let at = rng.below(n);
        let who = format!("svc-{}", rng.names(1).remove(0));
        let rows: Vec<_> = (0..n)
            .map(|i| {
                let by = if i == at { quoted(&who) } else { "null".into() };
                vec![
                    ("session", quoted(&format!("sess_k{i}"))),
                    ("user", quoted(&format!("u{}", 3 * i))),
                    ("revoked_by", by),
                ]
            })
            .collect();
        let query = json_query(
            vec![("session", Value::String(format!("sess_k{at}")))],
            Pick::Field("revoked_by"),
        );
        Gen {
            evidence: Some(format!("\"revoked_by\": \"{who}\"")),
            ..task(
                id,
                "evidence_drop",
                (
                    "json",
                    "exact",
                    &[&["drop-evidence"][..], &JSON_ARMS].concat(),
                ),
                pretty(&rows),
                (format!("Who revoked session sess_k{at}?"), who, query),
            )
        }
    } else {
        let alert = format!("{}-latency-p99", ADJ[rng.below(12)]);
        let step = |i: usize| format!("2026-09-30T12:{i:02}:00Z");
        let mut lines: Vec<String> = (0..n)
            .map(|i| format!("{} deploy step {i} ok", step(i)))
            .collect();
        lines.push(format!("{} rollback triggered by alert={alert}", step(n)));
        lines.push(format!("{} deploy step rollback ok", step(n + 1)));
        let query = scan("rollback triggered by", "alert=", " ");
        Gen {
            evidence: Some("rollback triggered by".into()),
            ..task(
                id,
                "evidence_drop",
                (
                    "log",
                    "exact",
                    &[&["drop-evidence"][..], &LINE_ARMS].concat(),
                ),
                lines.join("\n") + "\n",
                ("Which alert triggered the rollback?".into(), alert, query),
            )
        }
    };
    Gen {
        control: Some("evidence_drop"),
        ..g
    }
}

/// Writes `<out>/manifest.json` and `<out>/inputs/<task-id>.<ext>`.
pub fn run(seed: u64, out: &Path) -> Result<(), String> {
    let mut rng = Rng(seed);
    let mut gens: Vec<Gen> = (0..94).map(|t| json_task(t, &mut rng)).collect();
    gens.extend((0..101).map(|t| log_task(t, &mut rng)));
    gens.extend((0..6).map(|k| incompressible(k, &mut rng)));
    gens.extend((0..6).map(|k| evidence_drop(k, &mut rng)));
    for g in &gens {
        let tag = |e: String| format!("task {}: {e}", g.id);
        check(&g.input, &g.query, &g.answer).map_err(tag)?;
        if g.category == "buried_evidence" && g.ext == "json" {
            check_buried(&g.input, &g.query).map_err(tag)?;
        }
    }
    let inputs = out.join("inputs");
    fs::create_dir_all(&inputs).map_err(|e| format!("{}: {e}", inputs.display()))?;
    let mut tasks = Vec::new();
    for g in &gens {
        let file = format!("{}.{}", g.id, g.ext);
        fs::write(inputs.join(&file), &g.input).map_err(|e| format!("{file}: {e}"))?;
        let mut t = json!({
            "id": g.id, "category": g.category, "input": format!("inputs/{file}"),
            "question": g.question, "answers": [g.answer], "rule": g.rule, "arms": g.arms,
        });
        for (key, value) in [("control", g.control), ("evidence", g.evidence.as_deref())] {
            if let Some(v) = value {
                t[key] = json!(v);
            }
        }
        tasks.push(t);
    }
    let doc = json!({ "generator": "deslag-eval generate", "seed": seed, "tasks": tasks });
    // ponytail: byte-identical output relies on serde_json's sorted Map (preserve_order off); enabling that feature breaks the regeneration test.
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n";
    let path = out.join("manifest.json");
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}
