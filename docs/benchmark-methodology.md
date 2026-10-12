# Benchmark methodology: paired task evaluation

This document predeclares the scoring rules and the non-inferiority criterion for the paired task evaluation in PLAN §14.4. It was committed before any evaluation run. A change to the scoring rules or to the criterion after results have been inspected needs a decision record in `docs/decisions/`.

The two-call plumbing smoke check of the `claude` backend is excluded from the analysis.

The harness is the `deslag-eval` binary in `crates/deslag-eval`. The corpus is `tests/evaluations/manifest.json`. Results are in [docs/benchmarks/evaluation.md](benchmarks/evaluation.md).

## Unit of analysis

One task is one paired observation. Each task runs once per arm. The analysis pairs the baseline run of a task with each compressed run of the same task. Repeated turns and repeated runs are never treated as independent samples. The report therefore takes one run per results file: one seed, one backend and one model, with each (task, arm) at most once. It refuses any other file with exit 2. A repeated run gets its own report.

## Arms

| Arm | Prompt payload |
| --- | --- |
| `baseline` | The original tool result. |
| `json-minify` | `deslag_core::minify_json` output. |
| `table-csv` | table-v1 encoder output, `deslag_core::table::encode`, with the row and depth limits of the evaluation limits, the default savings thresholds of `TableOptions` and the estimate token counter. |
| `table-mdkv` | `deslag_core::table::render_markdown_kv` output with the row and depth limits of the evaluation limits, the default savings thresholds of `TableOptions` and the estimate token counter. Evaluation only. |
| `table-v2` | The json-table transform as `deslag_core::compress` ships it, with only `json_table` on, the estimate token counter and the evaluation limits. The payload is table-v1 or table-v2 text. |
| `lines-v1` | lines-v1 output of `deslag_core::compress` with only `exact_line_runs` on and the evaluation limits. |
| `lines-v2` | lines-v2 output of `deslag_core::compress` with only `line_templates` on and the evaluation limits. |
| `drop-evidence` | The original with every line that contains the task's `evidence` substring removed. Negative control only. |

The evaluation limits are the limits of `FIRST_SLICE_JSON_MINIFY_CONTEXT`: no input byte limits, at most 20000 table rows and a JSON depth of 128. The `table-csv` and `table-mdkv` arms share the row and depth limits, so both formats accept the same inputs. The `table-csv` versus `table-mdkv` read-back comparison therefore uses the same table-v1 encoding scope. The `table-csv` and `table-v2` arm definitions were added before any live evaluation run. The scoring rules and the non-inferiority criterion are unchanged.

Every task runs the `baseline` arm. A task lists the other arms that apply to it. When a transform declines the input, the arm's payload equals the baseline payload. The run still happens and its result line has `variant_unchanged: true`. Such pairs stay in the analysis, because the deployed transform would decline the same input.

## Prompt and model settings

Every run sends this system prompt:

```text
You answer questions about a tool result. Use only the tool result. Reply with the answer only, on one line, with no explanation. If the tool result does not contain the answer, reply NOT_FOUND.
```

Every run sends this user prompt, with `{payload}` replaced by the arm's payload and `{question}` by the task's question:

```text
<tool_result>
{payload}
</tool_result>

Question: {question}
```

The prompt does not explain table-v1, Markdown-KV, lines-v1 or lines-v2. A deployed hook does not explain them either.

One evaluation run uses one model. The result lines record the model id and the client version. `claude -p` has no temperature setting. Tools are disabled (`--tools ""`). Each run is a single turn.

## Scoring

The harness normalizes the model answer and every accepted answer the same way:

1. Trim surrounding whitespace.
2. Collapse each internal whitespace run to one space.
3. Strip one pair of surrounding double quotes, single quotes or backticks.
4. Strip one trailing period.

Each task names one rule:

| Rule | Success when |
| --- | --- |
| `exact` | The normalized answer equals a normalized accepted answer, case-sensitive. |
| `exact_ci` | As `exact`, case-insensitive. |
| `set` | The answer, split on commas with each item normalized and empty items dropped, equals an accepted answer split the same way, as a set. Both the whole answer and each item may carry one pair of quotes or backticks. Case-sensitive. Order does not matter. |
| `number` | The normalized answer parses as a 64-bit float and differs from an accepted answer by at most the task's `tolerance` (default 0). The comparison adds an epsilon of `4 * f64::EPSILON * max(1, abs(answer), abs(accepted))` to the tolerance, a few units of float rounding, so 1.1 against 1.0 with tolerance 0.1 is within tolerance. A non-finite value (infinity or NaN) never matches. |

Success is binary. Each failure has one error kind:

| Error kind | Meaning |
| --- | --- |
| `wrong` | A non-empty answer that scores as a failure. This is an exact-value or evidence error. |
| `abstain` | The normalized answer is empty or equals `NOT_FOUND`, case-insensitive. |
| `backend` | The backend call failed or reported an error. |

All three kinds count as failures. The report lists them separately.

## Non-inferiority criterion

The criterion is the PLAN §14.4 starting policy: the lower one-sided 95% confidence bound on compressed-minus-baseline success must exceed -2 percentage points.

The method is the Agresti-Min adjusted Wald interval for matched proportions. For one arm, the paired table over `n` tasks has:

- `b`: baseline succeeded, arm failed.
- `c`: baseline failed, arm succeeded.

Add 0.5 to each of the four cells. Then:

```text
n' = n + 2
d' = (c - b) / n'
se = sqrt(((b + 0.5) + (c + 0.5) - (c - b)^2 / n') / n'^2)
lower = d' - 1.6449 * se
upper = d' + 1.6449 * se
```

The arm passes when `lower > -0.02`. The verdict is `inferior` when `upper < -0.02`, so inferiority beyond the margin is shown at the same one-sided 95% level. Otherwise the verdict is `not shown`. A `not shown` verdict means the data cannot decide between non-inferiority and inferiority. An arm with zero paired non-control tasks (n = 0) gets the verdict `no data`.

With zero discordant pairs, `lower = -1.6449 / (n + 2)`. That exceeds -0.02 only for `n >= 81`. With fewer than 81 paired tasks, an arm passes only when discordance favors it. For example, `n = 20, b = 0, c = 3` gives `lower` of about -0.005 and passes. The criterion sets no minimum sample size. The initial corpus is a pilot, so an arm that roughly ties the baseline on it gets `not shown`.

The criterion is evaluated per arm over all non-control tasks that list the arm. The report also gives the paired counts and the bound per task category. Control tasks and the `drop-evidence` arm are excluded from this analysis.

## Secondary measures

Each result line records, when the backend reports them: input, output, cache-read and cache-creation tokens, cost in USD, backend duration in ms and the number of turns. It also records the payload bytes, the estimated payload tokens (`deslag-estimate-v1`) and `wall_ms`, the harness wall clock around each backend call. The mock records `wall_ms` as 0, so its report depends only on the manifest and the seed. Reported usage and estimated tokens are different measurements and are never mixed.

The harness never retries a call. A call that the client answers with an error, a failed exit or malformed output is recorded with error kind `backend`, so the retry count is always zero. A failure to start the client or to wait for it aborts the run with exit 2, and the partial results file is kept. Tool-call count does not apply, because the corpus is single-turn read-back with tools disabled. Multi-turn agentic tasks with real tools are a follow-up.

## Corpus and controls

Each task in `tests/evaluations/manifest.json` has `id`, `category`, `input` (a path relative to the manifest), `question`, `answers`, `rule`, an optional `tolerance` (a number >= 0), `arms`, an optional `control` and, for the evidence-drop control, `evidence`. A `control`, when present, is `incompressible` or `evidence_drop`. Only a task with control `evidence_drop` may list the `drop-evidence` arm. Every accepted answer of a `number` task parses as a finite number. The manifest is trusted and committed, so an input path may also be absolute. All content is synthetic. No real user content is exported.

The categories are the PLAN §14.4 task kinds: `api_query`, `code_search`, `failure_diagnosis`, `issue_triage`, `logs`, `aggregation`, `exact_id` and `buried_evidence`. A control task uses its control kind (`incompressible` or `evidence_drop`) as its category. A `buried_evidence` task puts the answer in the middle or at the end of a large result. At least one input has 100 or more rows.

Two control kinds validate the harness:

- `incompressible`: inputs that every listed transform declines. Every arm equals baseline. Any paired difference measures run-to-run noise of the model.
- `evidence_drop`: the `drop-evidence` arm removes the answer-carrying line. It must score below baseline. This shows that the harness detects evidence loss. For the `drop-evidence` arm, the report shows `baseline failed` when no baseline succeeded, `evidence loss detected` when the arm succeeded on fewer pairs than baseline, and `evidence loss not detected` otherwise. A transform arm on an `evidence_drop` task must keep the evidence. The report shows `evidence kept` when no pair has a successful baseline and a failed arm and at least one baseline succeeded. It shows `baseline failed` when no baseline succeeded. Otherwise it shows `a transform lost evidence`.

## Backends

The `mock` backend is offline and deterministic. It answers with the first accepted answer when that answer occurs verbatim in the payload, else `NOT_FOUND`. For a `set` task, every item must occur. The mock measures evidence presence only. It is plumbing evidence for the harness only.

The `claude` backend runs `claude -p` in a throwaway directory with a project `.claude/settings.json`, `--setting-sources project`, `--strict-mcp-config`, `--no-session-persistence`, `--tools ""` and the fixed system prompt. The prompt goes on stdin. It loads no user or local settings files, no MCP servers and no hooks. The client still uses the user's credentials and may update its own state file.

## Randomization

The run order is a seeded shuffle (splitmix64, Fisher-Yates) over all (task, arm) pairs. Each result line records the seed and its position in the run order.

## Cost controls

- The `claude` backend refuses to start unless `DESLAG_EVAL_LIVE=1` is set and both `--model` and `--budget-usd` are given.
- Every call passes `--max-budget-usd` with the per-run cap (`--per-run-usd`, default 0.30).
- Before each call, the runner stops when the spent amount plus the per-run cap would exceed `--budget-usd`. A call that reports no cost, or a negative or non-finite cost, counts as the full per-run cap, and the report treats such a cost as unreported.
- The client checks `--max-budget-usd` only after a turn, so one call can exceed its cap. The total can then exceed `--budget-usd` by that excess. The runner stops with exit 2 after any call whose reported cost exceeds `--per-run-usd`. The partial results file is kept.
- `run` refuses an existing `--out` path with exit 2, so a rerun never overwrites results.
- `deslag-eval estimate` prints, before any money is spent, an estimated cost of a full live run under stated assumptions (estimated tokens, assumed prices and assumed output tokens) and runs times `--per-run-usd`. That product is a cap only if every call stays within `--per-run-usd`. It refuses a negative or non-finite price and a `--per-run-usd` that is not above 0.
- CI runs only `cargo test`, which uses the mock backend. CI never sets `DESLAG_EVAL_LIVE`.

## Table format read-back

The `table-csv` and `table-mdkv` arms run on the same tasks. The report gives per-format accuracy for both, over non-control tasks that ran both arms. The CSV form cannot become a default until a live comparison exists. The DF-025 release gate (#22) owns the default decision.
