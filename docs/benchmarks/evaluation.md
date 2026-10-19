# Paired task evaluation

The method, the scoring rules and the non-inferiority criterion are predeclared in [benchmark-methodology.md](../benchmark-methodology.md). The corpus is `tests/evaluations/manifest.json`, which includes the generated corpus `tests/evaluations/generated/manifest.json` (seed 7700). It holds 228 synthetic tasks: the 21 pilot tasks (17 in the PLAN §14.4 categories, 2 incompressible controls, 2 evidence-drop controls) and 207 generated tasks (195 in the categories, 6 incompressible controls, 6 evidence-drop controls). It has 935 (task, arm) runs.

This file holds live run 1 (pilot corpus), the predeclared live run 2 and its report (full corpus), the predeclared live run 3 and its report (run-3 corpus), a two-call live plumbing smoke check and the offline mock dry runs (full corpus and run-3 corpus). The run-3 corpus is the standalone manifest `tests/evaluations/run3/manifest.json` (generator seed 10600, set `table`). It holds 222 synthetic tasks (210 JSON tasks, 6 incompressible controls, 6 evidence-drop controls) and 447 runs: baseline, `table-v2`, and `drop-evidence` on the evidence-drop controls.

## Status

| Field | Value |
| --- | --- |
| Live model results | live run 1: `claude-haiku-5-5`, seed 16, Claude Code 2.1.296, 90 runs, reported total USD 0.0443; live run 2: `claude-haiku-5-5`, seed 77, Claude Code 2.1.303, 935 runs, reported total USD 0.2888; live run 3: `claude-haiku-5-5`, seed 106, Claude Code 2.1.303, 447 runs, reported total USD 0.1478 |
| Live run 1 verdicts | every arm is `not shown`; no arm is `inferior`; no arm passed |
| Corpus size per arm | live run 1 (pilot): 4 to 12 non-control pairs; with zero discordant pairs an arm needs at least 81 to pass; below that an arm passes only when discordance favors it, and the observed favorable discordance (`lines-v1`, b = 0, c = 1) was insufficient |
| Live run 1 failures | the only transform-arm failure, `lines-v2` on `buried-artifact-sha`, also failed in baseline |
| Live run 1 controls | `drop-evidence` lost the evidence on both evidence-drop tasks; every arm left both incompressible tasks unchanged |
| table-v1 CSV vs Markdown-KV | live run 1: 11/11 for both formats; one run on one model cannot rank them, and CSV does not become a default from it; live run 2: table-csv 99/105, Markdown-KV 102/105, neither arm passes |
| Corpus size per arm (full corpus) | non-control pairs: json-minify 106, table-csv 105, table-mdkv 105, table-v2 105, lines-v1 106, lines-v2 105; changed non-control pairs (the transform changed the payload): json-minify 106, table-csv 104, table-mdkv 104, table-v2 105, lines-v1 104, lines-v2 105; 935 runs |
| Live run 2 | `json-minify` passes (b = 1, c = 5, lower bound -0.3 pp); every other arm is `not shown`; no arm is `inferior`; `drop-evidence` lost the evidence on all 8 evidence-drop tasks; no transform becomes a default from it |
| Live run 3 | `table-v2` is `not shown` on 210 non-control pairs (b = 4, c = 3, lower bound -2.7 pp); it is not `inferior`; paired Claude prompt-token saving 33.0%; `drop-evidence` lost the evidence on all 6 evidence-drop tasks; `table-v2` does not become a default from it; run-3 corpus generated with seed 10600 and set `table` (`tests/evaluations/run3/manifest.json` sha256 `2ecb581995d8b615a1d3ea580835c24a3ff1f3e196f1701e6d81405bf66aedc1`), mock output, `variants` and `estimate` outputs reproduced on it at commit 93d8699 |
| Mock dry run | below, full corpus and run-3 corpus, plumbing evidence only |
| Generated | live run 1 at commit 2627754 (v0.1.0-alpha.2), 2026-10-12; live run 2 at commit 85b9eed (v0.1.0-alpha.4), 2026-10-19; live run 3 at commit 93d8699, 2026-10-19; mock output reproduced on the full corpus generated with seed 7700 at commit e159c9d (`tests/evaluations/generated/manifest.json` sha256 `e620eeec695f7a909b58eb4e087d22e21241286f2681c5b824a820588171d848`) |

## Live runs

Run 1: one model, one seed, every (task, arm) pair of the 21-task pilot corpus (the manifest before the generated corpus was included). Its numbers stay as recorded. Its report is in [Live run 1](#live-run-1). The command needs authorized Claude credentials and spends money. CI never runs it. The command checks out commit 2627754 first, because the current manifest includes the generated corpus.

```sh
git checkout 2627754
mkdir -p tmp/eval
DESLAG_EVAL_LIVE=1 cargo run -q -p deslag-eval --locked -- run \
  --manifest tests/evaluations/manifest.json \
  --backend claude --model claude-haiku-5-5 --seed 16 \
  --budget-usd 3.00 --per-run-usd 0.30 \
  --out tmp/eval/live-claude-haiku-5-5-seed16.jsonl
cargo run -q -p deslag-eval --locked -- report tmp/eval/live-claude-haiku-5-5-seed16.jsonl
```

Estimated cost of run 1 (pilot corpus, 90 runs) as recorded before the run, with default assumptions. The estimate rests on estimated tokens, assumed output tokens and conservative assumed prices that may exceed the published price of the model. Runs times `--per-run-usd` is a cap only if every call stays within `--per-run-usd`. Before each call, the runner stops when the spent amount plus the per-run cap of USD 0.30 would exceed `--budget-usd 3.00`. Each call passes `--max-budget-usd 0.30`, but the client checks it only after a turn. One call can therefore exceed its cap, and the total can exceed USD 3.00 by that excess. The runner stops with exit 2 after any call that reports more than USD 0.30 and keeps the partial results file. `run` refuses an existing `--out` path.

| Field | Value |
| --- | --- |
| Runs (task, arm) | 90 |
| Estimated input tokens (deslag-estimate-v1, incl. 2000 overhead per run) | 337594 |
| Assumed output tokens per run | 256 |
| Assumed prices, USD per Mtok | input 3 (x1.25 cache write), output 15 |
| Estimated cost | USD 1.61 |
| Cap if every call stays within --per-run-usd (90 runs at 0.3) | USD 27.00 |

### Live run 2 (predeclared)

Run 2 is predeclared here before any live call on the full corpus. It uses the full corpus: the 21 pilot tasks plus 207 generated tasks (94 JSON, 101 log, 6 incompressible controls and 6 evidence-drop controls), 228 tasks and 935 runs. The generated corpus (seed 7700), the generator and the manifest are committed and frozen before this run. No task is changed after live results. The scoring rules and the criterion are those of the methodology. The paired Claude prompt-token savings are a secondary measure and never decide pass or fail.

```sh
mkdir -p tmp/eval
DESLAG_EVAL_LIVE=1 cargo run -q -p deslag-eval --locked -- run \
  --manifest tests/evaluations/manifest.json \
  --backend claude --model claude-haiku-5-5 --seed 77 \
  --budget-usd 3.00 --per-run-usd 0.30 \
  --out tmp/eval/live-claude-haiku-5-5-seed77.jsonl
cargo run -q -p deslag-eval --locked -- report tmp/eval/live-claude-haiku-5-5-seed77.jsonl
```

The run-1 defaults cost more than USD 3.00 on this corpus. Run 2 therefore states its own assumptions. Each is above the live run 1 measurement: about 740 overhead tokens per call (1000 assumed), about 70 output tokens per run (128 assumed), and an effective price of about 1/19 of the default price (prices of 1 and 5 USD per Mtok assumed, about 6 times the observed effective price). The defaults stay unchanged.

Estimate with the run-2 assumptions, from `cargo run -q -p deslag-eval --locked -- estimate --manifest tests/evaluations/manifest.json --input-usd-per-mtok 1 --output-usd-per-mtok 5 --overhead-tokens 1000 --output-tokens 128`:

| Field | Value |
| --- | --- |
| Runs (task, arm) | 935 |
| Estimated input tokens (deslag-estimate-v1, incl. 1000 overhead per run) | 1590323 |
| Assumed output tokens per run | 128 |
| Assumed prices, USD per Mtok | input 1 (x1.25 cache write), output 5 |
| Estimated cost | USD 2.59 |
| Cap if every call stays within --per-run-usd (935 runs at 0.3) | USD 280.50 |

Estimate with the default assumptions, from the same command without the assumption flags:

| Field | Value |
| --- | --- |
| Runs (task, arm) | 935 |
| Estimated input tokens (deslag-estimate-v1, incl. 2000 overhead per run) | 2525323 |
| Assumed output tokens per run | 256 |
| Assumed prices, USD per Mtok | input 3 (x1.25 cache write), output 15 |
| Estimated cost | USD 13.06 |
| Cap if every call stays within --per-run-usd (935 runs at 0.3) | USD 280.50 |

The default estimate exceeds `--budget-usd 3.00`. The run-2 estimate of USD 2.59 is below USD 2.70, the point at which the runner stops (the budget minus the per-run cap). Before each call, the runner stops when the spent amount plus the per-run cap would exceed the budget.

After a run, add its report as a new section above the mock dry run, with the client version and the total spend, and update the Status table.

### Live run 3 (predeclared)

Run 3 is predeclared here before any live call on the run-3 corpus. It decides `table-v2`, which live run 2 left at `not shown` with 105 pairs. `table-v2` is the primary arm of this run.

The run-3 corpus is `tests/evaluations/run3/manifest.json`. It is a standalone manifest without an include. The generator wrote it with `cargo run -q -p deslag-eval --locked -- generate --seed 10600 --set table --out tests/evaluations/run3`. The manifest sha256 is `2ecb581995d8b615a1d3ea580835c24a3ff1f3e196f1701e6d81405bf66aedc1`. The corpus holds 222 synthetic tasks: 210 JSON tasks, 6 incompressible controls and 6 evidence-drop controls. Its task ids start with `r3-`, so its results cannot be confused with run 2. The JSON tasks use the same slot logic as the run-2 generator and therefore the same share of hard tasks: buried evidence (10 tasks), exact ids, near-duplicate decoy rows, null vs missing fields, number lexemes and deep rows. The generator self-checks every answer against the uncompressed payload.

The manifest arms restrict the run to the baseline and `table-v2`. Every non-control task and every incompressible control lists `table-v2` only. The 3 JSON evidence-drop controls list `drop-evidence` and `table-v2`. The 3 log evidence-drop controls list `drop-evidence` only. The run has 447 (task, arm) runs: 222 baseline, 219 `table-v2` and 6 `drop-evidence`. `table-v2` has 210 non-control pairs, and the transform changes the payload in all 210.

The corpus, the generator and the manifest are committed and frozen before this run. No task is changed after live results. The scoring rules and the non-inferiority criterion are those of the methodology and stay unchanged. The paired Claude prompt-token savings are a secondary measure and never decide pass or fail. Run 3 runs once. It is not rerun without a new operator approval.

```sh
mkdir -p tmp/eval
DESLAG_EVAL_LIVE=1 cargo run -q -p deslag-eval --locked -- run \
  --manifest tests/evaluations/run3/manifest.json \
  --backend claude --model claude-haiku-5-5 --seed 106 \
  --budget-usd 1.00 --per-run-usd 0.30 \
  --out tmp/eval/live-claude-haiku-5-5-seed106.jsonl
cargo run -q -p deslag-eval --locked -- report tmp/eval/live-claude-haiku-5-5-seed106.jsonl
```

Run 3 states its own assumptions from the live run 2 measurement. The run-2 estimate of USD 2.59 used prices of 1 and 5 USD per Mtok, 1000 overhead tokens per call and 128 output tokens per run. The reported spend was USD 0.2888, so the estimate was about 9 times the spend. The client reported 1441504 input tokens against 1590323 estimated, so the 1000-token overhead held. It reported about 80 output tokens per run, so 128 output tokens stay. Run 3 assumes half the run-2 prices: 0.5 and 2.5 USD per Mtok. With these prices the estimate is still about 4.5 times the spend that the run-2 ratio predicts (about USD 0.13). The defaults stay unchanged.

Estimate with the run-3 assumptions, from `cargo run -q -p deslag-eval --locked -- estimate --manifest tests/evaluations/run3/manifest.json --input-usd-per-mtok 0.5 --output-usd-per-mtok 2.5 --overhead-tokens 1000 --output-tokens 128 --per-run-usd 0.30`:

| Field | Value |
| --- | --- |
| Runs (task, arm) | 447 |
| Estimated input tokens (deslag-estimate-v1, incl. 1000 overhead per run) | 704008 |
| Assumed output tokens per run | 128 |
| Assumed prices, USD per Mtok | input 0.5 (x1.25 cache write), output 2.5 |
| Estimated cost | USD 0.58 |
| Cap if every call stays within --per-run-usd (447 runs at 0.3) | USD 134.10 |

Estimate with the default assumptions, from `cargo run -q -p deslag-eval --locked -- estimate --manifest tests/evaluations/run3/manifest.json`:

| Field | Value |
| --- | --- |
| Runs (task, arm) | 447 |
| Estimated input tokens (deslag-estimate-v1, incl. 2000 overhead per run) | 1151008 |
| Assumed output tokens per run | 256 |
| Assumed prices, USD per Mtok | input 3 (x1.25 cache write), output 15 |
| Estimated cost | USD 6.03 |
| Cap if every call stays within --per-run-usd (447 runs at 0.3) | USD 134.10 |

The default estimate exceeds `--budget-usd 1.00`. The run-3 estimate of USD 0.58 is below USD 1.00 and below USD 0.70, the point at which the runner stops (the budget minus the per-run cap). Before each call, the runner stops when the spent amount plus the per-run cap would exceed the budget. The operator authorized at most USD 1.00 of live spend for run 3.

The run-3 prices are calibrated from the run-2 reported spend. With the run-2 prices of 1 and 5 USD per Mtok, the same command estimates USD 1.17. The runner also charges the per-run cap of USD 0.30 for each run without a valid reported cost, so three such runs stop it. A budget stop or a per-run cap stop ends run 3, and run 3 is not rerun. A stopped run is reported as it is, from the pairs where both runs completed. Below 200 non-control pairs, the `table-v2` verdict is reported but is not decisive for this issue.

The run-3 mock dry run is in [Mock dry run (run-3 corpus)](#mock-dry-run-run-3-corpus). The run-3 payload sizes are in [Run-3 payload sizes](#run-3-payload-sizes).

After the run, add its report as a new section above the mock dry run, with the commit, the client version, the total spend, both savings measures and the results-file sha256, and update the Status table.

## Live plumbing smoke check

On 2026-10-12, 2 live calls checked the `claude` backend plumbing: task `api-repo-branch`, arms `baseline` and `json-minify`, model `claude-haiku-5-5`, client 2.1.296, seed 16, `--per-run-usd 0.30`, `--budget-usd 0.60`. Both calls answered correctly. The total reported cost was USD 0.0004. The cache-creation tokens were 918 and 868 against payload estimates of 178 and 130. The per-call overhead was therefore about 740 tokens, below the 2000-token overhead that the estimate assumes. This is plumbing evidence only, and the analysis excludes it.

## Live run 1

On 2026-10-12, the command in [Live runs](#live-runs) ran at commit 2627754 (v0.1.0-alpha.2) with model `claude-haiku-5-5`, Claude Code 2.1.296 and seed 16. All 90 runs completed. The reported total spend was USD 0.0443. The results file `tmp/eval/live-claude-haiku-5-5-seed16.jsonl` is not committed. Its sha256 is `52afddc83b72f430eb20eb2ebeccccc198475eab471063f48449fe2bc94ffa3a`.

Every arm is `not shown`. The pilot corpus has 4 to 12 non-control pairs per arm, and with zero discordant pairs an arm needs at least 81 pairs to pass. Below that size an arm passes only when discordance favors it. The only favorable discordance in this run, `lines-v1` with b = 0 and c = 1, was insufficient: its lower bound is -17.7 pp. No arm was `inferior`. Apart from the `drop-evidence` control, the only arm failure was `lines-v2` on `buried-artifact-sha`, and the baseline also failed that task. `lines-v1` answered it correctly, which is the single discordant pair (c = 1). Both failed answers contain the accepted hash but score `wrong` under `exact_ci`: the baseline added a sentence, and `lines-v2` added a `sha256=` prefix. The discordance is therefore an answer-format effect, not evidence loss. The controls behaved as designed: `drop-evidence` lost the evidence on both evidence-drop tasks, and every arm left both incompressible tasks unchanged. This run does not make any transform a default.

The pre-run estimate of USD 1.61 overstated the cost. It assumed 337594 input tokens in total and 256 output tokens per run. The client reported 251775 input tokens (180 uncached, 48364 cache read, 203231 cache creation) and 6270 output tokens, about 70 per run. Priced at the assumed rates (cache reads excluded), the reported tokens cost about USD 0.86. The reported spend of USD 0.0443 is about 1/19 of that, so the effective prices were roughly 1/19 of the assumed prices.

Report output of `cargo run -q -p deslag-eval --locked -- report tmp/eval/live-claude-haiku-5-5-seed16.jsonl`, verbatim:

### Run

| Field | Value |
| --- | --- |
| backend | claude |
| model | claude-haiku-5-5 |
| client version | 2.1.296 (Claude Code) |
| seed | 16 |
| result lines | 90 |
| runs without a baseline partner | 0 |
| reported cost, USD | 0.0443 |

### Non-inferiority per arm

Non-control tasks. Agresti-Min lower one-sided 95% bound; an arm passes when the bound exceeds -2.0 pp and is inferior when the upper one-sided 95% bound is below -2.0 pp. b: only baseline succeeded; c: only the arm succeeded.

| Arm | Pairs | Unchanged | b | c | Baseline success | Arm success | Difference pp | Lower bound pp | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| json-minify | 12 | 0 | 0 | 0 | 12/12 (100.0%) | 12/12 (100.0%) | +0.0 | -11.7 | not shown |
| lines-v1 | 5 | 2 | 0 | 1 | 4/5 (80.0%) | 5/5 (100.0%) | +20.0 | -17.7 | not shown |
| lines-v2 | 4 | 0 | 0 | 0 | 3/4 (75.0%) | 3/4 (75.0%) | +0.0 | -27.4 | not shown |
| table-csv | 11 | 1 | 0 | 0 | 11/11 (100.0%) | 11/11 (100.0%) | +0.0 | -12.7 | not shown |
| table-mdkv | 11 | 1 | 0 | 0 | 11/11 (100.0%) | 11/11 (100.0%) | +0.0 | -12.7 | not shown |
| table-v2 | 11 | 0 | 0 | 0 | 11/11 (100.0%) | 11/11 (100.0%) | +0.0 | -12.7 | not shown |

### Per category

| Category | Arm | Pairs | Baseline success | Arm success | Difference pp | Lower bound pp | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| aggregation | json-minify | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| aggregation | table-csv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| aggregation | table-mdkv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| aggregation | table-v2 | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| api_query | json-minify | 3 | 3/3 (100.0%) | 3/3 (100.0%) | +0.0 | -32.9 | not shown |
| api_query | table-csv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| api_query | table-mdkv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| api_query | table-v2 | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| buried_evidence | json-minify | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| buried_evidence | lines-v1 | 1 | 0/1 (0.0%) | 1/1 (100.0%) | +100.0 | -37.5 | not shown |
| buried_evidence | lines-v2 | 1 | 0/1 (0.0%) | 0/1 (0.0%) | +0.0 | -54.8 | not shown |
| buried_evidence | table-csv | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| buried_evidence | table-mdkv | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| buried_evidence | table-v2 | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| code_search | json-minify | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| code_search | lines-v1 | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| code_search | table-csv | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| code_search | table-mdkv | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| code_search | table-v2 | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| exact_id | json-minify | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| exact_id | table-csv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| exact_id | table-mdkv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| exact_id | table-v2 | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| failure_diagnosis | json-minify | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| failure_diagnosis | lines-v1 | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| failure_diagnosis | lines-v2 | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| failure_diagnosis | table-csv | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| failure_diagnosis | table-mdkv | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| failure_diagnosis | table-v2 | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| issue_triage | json-minify | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| issue_triage | table-csv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| issue_triage | table-mdkv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| issue_triage | table-v2 | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| logs | lines-v1 | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| logs | lines-v2 | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |

### Table format read-back

Non-control tasks that ran both table arms. Control tasks are excluded.

| Format | Arm | Runs | Success | Wrong | Abstain | Backend error |
| --- | --- | --- | --- | --- | --- | --- |
| table-v1 CSV | table-csv | 11 | 11/11 (100.0%) | 0 | 0 | 0 |
| Markdown-KV | table-mdkv | 11 | 11/11 (100.0%) | 0 | 0 | 0 |

### Controls

| Control | Arm | Pairs | Baseline success | Arm success | Unchanged | Discordant | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| evidence_drop | drop-evidence | 2 | 2/2 (100.0%) | 0/2 (0.0%) | 0 | 2 | evidence loss detected |
| evidence_drop | lines-v2 | 1 | 1/1 (100.0%) | 1/1 (100.0%) | 0 | 0 | evidence kept |
| incompressible | json-minify | 2 | 2/2 (100.0%) | 2/2 (100.0%) | 2 | 0 | all variants unchanged |
| incompressible | lines-v1 | 2 | 2/2 (100.0%) | 2/2 (100.0%) | 2 | 0 | all variants unchanged |
| incompressible | lines-v2 | 2 | 2/2 (100.0%) | 2/2 (100.0%) | 2 | 0 | all variants unchanged |
| incompressible | table-csv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | 2 | 0 | all variants unchanged |
| incompressible | table-mdkv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | 2 | 0 | all variants unchanged |
| incompressible | table-v2 | 2 | 2/2 (100.0%) | 2/2 (100.0%) | 2 | 0 | all variants unchanged |

### Per arm

All tasks, controls included. Payload tokens are estimates (deslag-estimate-v1). The columns from Input tokens to Turns are backend-reported sums; n/a means not reported, and a negative or non-finite value counts as not reported. Wall ms is the summed harness wall clock of the backend calls.

| Arm | Runs | Success | Wrong | Abstain | Backend error | Payload tokens (estimate) | Input tokens | Output tokens | Cache read | Cache creation | Cost USD | Duration ms | Turns | Wall ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| baseline | 21 | 20/21 (95.2%) | 1 | 0 | 0 | 47482 | 42 | 1209 | 2597 | 72022 | 0.0150 | 23359 | 21 | 49591 |
| json-minify | 14 | 14/14 (100.0%) | 0 | 0 | 0 | 24326 | 28 | 1052 | 1551 | 39869 | 0.0085 | 15669 | 14 | 33007 |
| table-csv | 13 | 13/13 (100.0%) | 0 | 0 | 0 | 16654 | 26 | 1108 | 18690 | 10170 | 0.0028 | 14895 | 13 | 31156 |
| table-mdkv | 13 | 13/13 (100.0%) | 0 | 0 | 0 | 24794 | 26 | 1041 | 5072 | 36124 | 0.0078 | 15611 | 13 | 32713 |
| table-v2 | 13 | 13/13 (100.0%) | 0 | 0 | 0 | 15473 | 26 | 1011 | 10888 | 16471 | 0.0039 | 13838 | 13 | 30103 |
| lines-v1 | 7 | 7/7 (100.0%) | 0 | 0 | 0 | 11834 | 14 | 377 | 8015 | 11782 | 0.0026 | 6929 | 7 | 15370 |
| lines-v2 | 7 | 6/7 (85.7%) | 1 | 0 | 0 | 9079 | 14 | 337 | 1551 | 13935 | 0.0030 | 6924 | 7 | 15701 |
| drop-evidence | 2 | 0/2 (0.0%) | 0 | 2 | 0 | 1136 | 4 | 135 | 0 | 2858 | 0.0006 | 2384 | 2 | 4991 |

End of report output.

### Paired payload savings

Derived from the results file. Non-control tasks only. Each arm payload is compared with the baseline payload of the same task. Tokens are `deslag-estimate-v1` estimates. The `lines-v1` row includes 2 unchanged pairs; the `table-csv` and `table-mdkv` rows include 1 each.

```sh
jq -rs '(map(select(.arm=="baseline"))|map({key:.task,value:.payload_tokens_est})|from_entries) as $b
| map(select(.arm!="baseline" and .control==null))
| group_by(.arm)[]
| {arm:.[0].arm, pairs:length, base:(map($b[.task])|add), arm_tok:(map(.payload_tokens_est)|add)}
| "| \(.arm) | \(.pairs) | \(.base) | \(.arm_tok) | \(.base-.arm_tok) | \((1000*(.base-.arm_tok)/.base|round)/10) |"' \
  tmp/eval/live-claude-haiku-5-5-seed16.jsonl
```

| Arm | Pairs | Baseline tokens (estimate) | Arm tokens (estimate) | Saved tokens | Saved % |
| --- | --- | --- | --- | --- | --- |
| json-minify | 12 | 33753 | 24217 | 9536 | 28.3 |
| lines-v1 | 5 | 12445 | 11725 | 720 | 5.8 |
| lines-v2 | 4 | 11587 | 8455 | 3132 | 27 |
| table-csv | 11 | 33575 | 16545 | 17030 | 50.7 |
| table-mdkv | 11 | 33575 | 24685 | 8890 | 26.5 |
| table-v2 | 11 | 33575 | 15364 | 18211 | 54.2 |

## Live run 2

On 2026-10-19, the command in [Live run 2 (predeclared)](#live-run-2-predeclared) ran at commit 85b9eed (v0.1.0-alpha.4) with model `claude-haiku-5-5`, Claude Code 2.1.303 and seed 77, on the full corpus (228 tasks, `tests/evaluations/generated/manifest.json` sha256 `e620eeec695f7a909b58eb4e087d22e21241286f2681c5b824a820588171d848`). All 935 runs completed with exit 0 and no backend error. The harness reported a total spend of USD 0.2888. The results file `tmp/eval/live-claude-haiku-5-5-seed77.jsonl` is not committed. Its sha256 is `9c7de55ccf5c85146102308f480b6b2813c3354fe2afec1f23c77c29aa315291`.

`json-minify` passes: b = 1, c = 5, and its lower bound of -0.3 pp is above -2.0 pp. Every other arm is `not shown`. Their lower bounds range from -6.5 pp to -2.7 pp, and no arm is `inferior`. `table-mdkv` and `table-v2` (b = 3, c = 5) are closest to the margin. A pass on one run, one model and one seed meets the predeclared criterion. It does not make `json-minify` a default; defaults belong to the config work.

The controls behaved as designed: `drop-evidence` lost the evidence on all 8 evidence-drop tasks, every arm kept the evidence on the evidence-drop tasks, and every arm left all 8 incompressible tasks unchanged.

Apart from the `drop-evidence` control, 34 runs failed: 9 baseline runs and 25 arm runs. 14 failed answers contain a self-correction, such as "wait, let me check", and 7 add a `sha256=` prefix to the accepted hash. Both forms score `wrong` under `exact_ci`, in the baseline as in the arms. The discordant pairs therefore mix answer-format effects with possible evidence effects; this report does not separate them.

The paired Claude prompt-token savings, the secondary measure, are positive for every arm: `table-v2` 78854 tokens (36.8%), `table-csv` 77353 (36.1%), `json-minify` 37373 (17.4%), `table-mdkv` 34271 (16.0%), `lines-v2` 23743 (13.3%) and `lines-v1` 7797 (4.3%). The percentages include the system prompt and client overhead of every call. For every arm, the `deslag-estimate-v1` payload saving is lower than the Claude saving, by 15.3% to 33.7%. The estimate therefore understates the saving, as the token fixture README predicts.

The pre-run estimate of USD 2.59 overstated the cost about 9 times. It assumed 1590323 input tokens and 128 output tokens per run. The client reported 1441504 input tokens (1870 uncached, 191972 cache read, 1247662 cache creation) and 74401 output tokens, about 80 per run.

Report output of `cargo run -q -p deslag-eval --locked -- report tmp/eval/live-claude-haiku-5-5-seed77.jsonl`, verbatim:

### Run

| Field | Value |
| --- | --- |
| backend | claude |
| model | claude-haiku-5-5 |
| client version | 2.1.303 (Claude Code) |
| seed | 77 |
| result lines | 935 |
| runs without a baseline partner | 0 |
| reported cost, USD | 0.2888 |

### Non-inferiority per arm

Non-control tasks. Agresti-Min lower one-sided 95% bound; an arm passes when the bound exceeds -2.0 pp and is inferior when the upper one-sided 95% bound is below -2.0 pp. b: only baseline succeeded; c: only the arm succeeded.

| Arm | Pairs | Unchanged | b | c | Baseline success | Arm success | Difference pp | Lower bound pp | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| json-minify | 106 | 0 | 1 | 5 | 101/106 (95.3%) | 105/106 (99.1%) | +3.8 | -0.3 | passes |
| lines-v1 | 106 | 2 | 4 | 2 | 102/106 (96.2%) | 100/106 (94.3%) | -1.9 | -5.9 | not shown |
| lines-v2 | 105 | 0 | 5 | 3 | 101/105 (96.2%) | 99/105 (94.3%) | -1.9 | -6.5 | not shown |
| table-csv | 105 | 1 | 5 | 4 | 100/105 (95.2%) | 99/105 (94.3%) | -1.0 | -5.8 | not shown |
| table-mdkv | 105 | 1 | 3 | 5 | 100/105 (95.2%) | 102/105 (97.1%) | +1.9 | -2.7 | not shown |
| table-v2 | 105 | 0 | 3 | 5 | 100/105 (95.2%) | 102/105 (97.1%) | +1.9 | -2.7 | not shown |

### Per category

| Category | Arm | Pairs | Baseline success | Arm success | Difference pp | Lower bound pp | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| aggregation | json-minify | 17 | 15/17 (88.2%) | 17/17 (100.0%) | +11.8 | -3.9 | not shown |
| aggregation | table-csv | 17 | 15/17 (88.2%) | 14/17 (82.4%) | -5.9 | -22.5 | not shown |
| aggregation | table-mdkv | 17 | 15/17 (88.2%) | 17/17 (100.0%) | +11.8 | -3.9 | not shown |
| aggregation | table-v2 | 17 | 15/17 (88.2%) | 16/17 (94.1%) | +5.9 | -11.9 | not shown |
| api_query | json-minify | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| api_query | table-csv | 16 | 16/16 (100.0%) | 16/16 (100.0%) | +0.0 | -9.1 | not shown |
| api_query | table-mdkv | 16 | 16/16 (100.0%) | 16/16 (100.0%) | +0.0 | -9.1 | not shown |
| api_query | table-v2 | 16 | 16/16 (100.0%) | 16/16 (100.0%) | +0.0 | -9.1 | not shown |
| buried_evidence | json-minify | 6 | 5/6 (83.3%) | 6/6 (100.0%) | +16.7 | -15.7 | not shown |
| buried_evidence | lines-v1 | 7 | 4/7 (57.1%) | 5/7 (71.4%) | +14.3 | -24.9 | not shown |
| buried_evidence | lines-v2 | 7 | 4/7 (57.1%) | 4/7 (57.1%) | +0.0 | -40.9 | not shown |
| buried_evidence | table-csv | 6 | 5/6 (83.3%) | 6/6 (100.0%) | +16.7 | -15.7 | not shown |
| buried_evidence | table-mdkv | 6 | 5/6 (83.3%) | 6/6 (100.0%) | +16.7 | -15.7 | not shown |
| buried_evidence | table-v2 | 6 | 5/6 (83.3%) | 6/6 (100.0%) | +16.7 | -15.7 | not shown |
| code_search | json-minify | 17 | 16/17 (94.1%) | 17/17 (100.0%) | +5.9 | -6.8 | not shown |
| code_search | lines-v1 | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| code_search | table-csv | 17 | 16/17 (94.1%) | 16/17 (94.1%) | +0.0 | -15.0 | not shown |
| code_search | table-mdkv | 17 | 16/17 (94.1%) | 16/17 (94.1%) | +0.0 | -15.0 | not shown |
| code_search | table-v2 | 17 | 16/17 (94.1%) | 17/17 (100.0%) | +5.9 | -6.8 | not shown |
| exact_id | json-minify | 17 | 16/17 (94.1%) | 16/17 (94.1%) | +0.0 | -15.0 | not shown |
| exact_id | table-csv | 17 | 16/17 (94.1%) | 15/17 (88.2%) | -5.9 | -22.5 | not shown |
| exact_id | table-mdkv | 17 | 16/17 (94.1%) | 15/17 (88.2%) | -5.9 | -22.5 | not shown |
| exact_id | table-v2 | 17 | 16/17 (94.1%) | 15/17 (88.2%) | -5.9 | -22.5 | not shown |
| failure_diagnosis | json-minify | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| failure_diagnosis | lines-v1 | 32 | 32/32 (100.0%) | 32/32 (100.0%) | +0.0 | -4.8 | not shown |
| failure_diagnosis | lines-v2 | 32 | 32/32 (100.0%) | 32/32 (100.0%) | +0.0 | -4.8 | not shown |
| failure_diagnosis | table-csv | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| failure_diagnosis | table-mdkv | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| failure_diagnosis | table-v2 | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| issue_triage | json-minify | 15 | 15/15 (100.0%) | 15/15 (100.0%) | +0.0 | -9.7 | not shown |
| issue_triage | table-csv | 15 | 15/15 (100.0%) | 15/15 (100.0%) | +0.0 | -9.7 | not shown |
| issue_triage | table-mdkv | 15 | 15/15 (100.0%) | 15/15 (100.0%) | +0.0 | -9.7 | not shown |
| issue_triage | table-v2 | 15 | 15/15 (100.0%) | 15/15 (100.0%) | +0.0 | -9.7 | not shown |
| logs | lines-v1 | 66 | 65/66 (98.5%) | 62/66 (93.9%) | -4.5 | -9.2 | not shown |
| logs | lines-v2 | 66 | 65/66 (98.5%) | 63/66 (95.5%) | -3.0 | -8.3 | not shown |

### Table format read-back

Non-control tasks that ran both table arms. Control tasks are excluded.

| Format | Arm | Runs | Success | Wrong | Abstain | Backend error |
| --- | --- | --- | --- | --- | --- | --- |
| table-v1 CSV | table-csv | 105 | 99/105 (94.3%) | 6 | 0 | 0 |
| Markdown-KV | table-mdkv | 105 | 102/105 (97.1%) | 3 | 0 | 0 |

### Controls

| Control | Arm | Pairs | Baseline success | Arm success | Unchanged | Discordant | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| evidence_drop | drop-evidence | 8 | 8/8 (100.0%) | 0/8 (0.0%) | 0 | 8 | evidence loss detected |
| evidence_drop | json-minify | 3 | 3/3 (100.0%) | 3/3 (100.0%) | 0 | 0 | evidence kept |
| evidence_drop | lines-v1 | 3 | 3/3 (100.0%) | 3/3 (100.0%) | 3 | 0 | evidence kept |
| evidence_drop | lines-v2 | 4 | 4/4 (100.0%) | 4/4 (100.0%) | 0 | 0 | evidence kept |
| evidence_drop | table-csv | 3 | 3/3 (100.0%) | 3/3 (100.0%) | 0 | 0 | evidence kept |
| evidence_drop | table-mdkv | 3 | 3/3 (100.0%) | 3/3 (100.0%) | 0 | 0 | evidence kept |
| evidence_drop | table-v2 | 3 | 3/3 (100.0%) | 3/3 (100.0%) | 0 | 0 | evidence kept |
| incompressible | json-minify | 8 | 8/8 (100.0%) | 8/8 (100.0%) | 8 | 0 | all variants unchanged |
| incompressible | lines-v1 | 8 | 8/8 (100.0%) | 8/8 (100.0%) | 8 | 0 | all variants unchanged |
| incompressible | lines-v2 | 8 | 8/8 (100.0%) | 8/8 (100.0%) | 8 | 0 | all variants unchanged |
| incompressible | table-csv | 8 | 8/8 (100.0%) | 8/8 (100.0%) | 8 | 0 | all variants unchanged |
| incompressible | table-mdkv | 8 | 8/8 (100.0%) | 8/8 (100.0%) | 8 | 0 | all variants unchanged |
| incompressible | table-v2 | 8 | 8/8 (100.0%) | 8/8 (100.0%) | 8 | 0 | all variants unchanged |

### Per arm

All tasks, controls included. Payload tokens are estimates (deslag-estimate-v1). The columns from Input tokens to Turns are backend-reported sums; n/a means not reported, and a negative or non-finite value counts as not reported. Wall ms is the summed harness wall clock of the backend calls.

| Arm | Runs | Success | Wrong | Abstain | Backend error | Payload tokens (estimate) | Input tokens | Output tokens | Cache read | Cache creation | Cost USD | Duration ms | Turns | Wall ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| baseline | 228 | 219/228 (96.1%) | 9 | 0 | 0 | 190626 | 456 | 14664 | 7567 | 405262 | 0.0885 | 327282 | 228 | 600427 |
| json-minify | 117 | 116/117 (99.1%) | 1 | 0 | 0 | 73653 | 234 | 11670 | 5357 | 182232 | 0.0424 | 175529 | 117 | 315901 |
| table-csv | 116 | 110/116 (94.8%) | 6 | 0 | 0 | 47094 | 232 | 12794 | 70310 | 75492 | 0.0222 | 189348 | 116 | 329340 |
| table-mdkv | 116 | 113/116 (97.4%) | 3 | 0 | 0 | 76653 | 232 | 10824 | 8045 | 181723 | 0.0419 | 179269 | 116 | 319086 |
| table-v2 | 116 | 113/116 (97.4%) | 3 | 0 | 0 | 45913 | 232 | 12592 | 80058 | 64243 | 0.0200 | 173917 | 116 | 311776 |
| lines-v1 | 117 | 111/117 (94.9%) | 6 | 0 | 0 | 78242 | 234 | 5171 | 16857 | 165560 | 0.0359 | 165235 | 117 | 305861 |
| lines-v2 | 117 | 111/117 (94.9%) | 6 | 0 | 0 | 67536 | 234 | 6152 | 3778 | 161782 | 0.0355 | 165873 | 117 | 306438 |
| drop-evidence | 8 | 0/8 (0.0%) | 0 | 8 | 0 | 4506 | 16 | 534 | 0 | 11368 | 0.0025 | 12086 | 8 | 21944 |

### Paired savings

Secondary measure; it never decides pass or fail. Non-control tasks; drop-evidence is a control arm and has no row. Saved is baseline minus arm, summed over the pairs. The estimate columns sum payload tokens (deslag-estimate-v1) over all pairs. The Claude columns sum backend-reported input, cache read and cache creation tokens over the pairs where both runs report all three; they include the system prompt and client overhead, which cancel in the difference but dilute the percentage. Estimate error % is the estimate saved on those same pairs minus the Claude saved, over the absolute Claude saved; positive means the estimate overstates the saving. A pair whose baseline or arm payload estimate is not a finite number of at least 0 is dropped from the whole row.

| Arm | Pairs | Saved tokens (estimate) | Saved % (estimate) | Pairs with Claude tokens | Baseline prompt tokens (Claude) | Arm prompt tokens (Claude) | Saved tokens (Claude) | Saved % (Claude) | Estimate error % |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| json-minify | 106 | 31643 | 30.5 | 106 | 215288 | 177915 | 37373 | 17.4 | -15.3 |
| lines-v1 | 106 | 5552 | 6.8 | 106 | 180305 | 172508 | 7797 | 4.3 | -28.8 |
| lines-v2 | 105 | 15744 | 19.4 | 105 | 178539 | 154796 | 23743 | 13.3 | -33.7 |
| table-csv | 105 | 57472 | 55.5 | 105 | 214368 | 137015 | 77353 | 36.1 | -25.7 |
| table-mdkv | 105 | 28525 | 27.6 | 105 | 214368 | 180097 | 34271 | 16.0 | -16.8 |
| table-v2 | 105 | 58653 | 56.7 | 105 | 214368 | 135514 | 78854 | 36.8 | -25.6 |

## Live run 3

On 2026-10-19, the command in [Live run 3 (predeclared)](#live-run-3-predeclared) ran once at commit 93d8699 (v0.1.0-alpha.4 plus 3 commits) with model `claude-haiku-5-5`, Claude Code 2.1.303 and seed 106, on the run-3 corpus (`tests/evaluations/run3/manifest.json` sha256 `2ecb581995d8b615a1d3ea580835c24a3ff1f3e196f1701e6d81405bf66aedc1`). All 447 runs completed with exit 0 and no backend error. The harness reported a total spend of USD 0.1478, within the authorized USD 1.00. The results file `tmp/eval/live-claude-haiku-5-5-seed106.jsonl` is not committed. Its sha256 is `af1034df33ddb3a2939076c1df32f0321d64ab9270c926f379a4a83b1aee8757`.

`table-v2` is `not shown`. It has 210 non-control pairs, so the run meets the predeclared size of 200 pairs and its verdict decides `table-v2` for this corpus, model and seed. b = 4 and c = 3: the baseline succeeded on 207 of 210 tasks and `table-v2` on 206. The difference is -0.5 pp and the lower bound is -2.7 pp, below the -2.0 pp margin. `table-v2` is not `inferior`. Run 2 gave the same lower bound of -2.7 pp with 105 pairs. This report does not pool the two runs, because the methodology does not predeclare pooling.

The controls behaved as designed: `drop-evidence` lost the evidence on all 6 evidence-drop tasks, `table-v2` kept the evidence on the 3 JSON evidence-drop tasks, and `table-v2` left all 6 incompressible tasks unchanged.

Apart from the `drop-evidence` control, 7 runs failed: 3 baseline runs and 4 `table-v2` runs. No task failed in both runs, so all 7 are discordant pairs. Every failed answer is a single value or list with no added text, so these failures are not answer-format effects. The 4 `table-v2` failures:

- `r3-json-002` and `r3-json-171`: id lookups in tables of 11 and 13 rows. The answer is the name of row 0. The matching row is row 3.
- `r3-json-088`: the answer is the near-duplicate decoy id, which differs from the accepted id in its last character.
- `r3-json-012`: the highest `amount_cents` among paid orders is 207. The answer is 183, the value of another paid order.

The 3 baseline failures are a near-duplicate decoy id (`r3-json-124`), a wrong service for the highest error rate (`r3-json-200`), and a null-coupon set that lists 2 of the 4 accepted ids (`r3-json-054`). This report does not establish a cause for the `table-v2` failures.

The paired Claude prompt-token saving of `table-v2`, the secondary measure, is 119079 tokens (33.0%). The percentage includes the system prompt and client overhead of every call. The `deslag-estimate-v1` payload saving is 87196 tokens (57.9% of the estimated payload tokens), 26.8% lower than the Claude saving.

The pre-run estimate of USD 0.58 overstated the cost about 4 times. It assumed 704008 input tokens and 128 output tokens per run. The client reported 631807 input tokens (896 uncached, 5774 cache read, 625137 cache creation) and 45210 output tokens, about 101 per run.

Report output of `cargo run -q -p deslag-eval --locked -- report tmp/eval/live-claude-haiku-5-5-seed106.jsonl`, verbatim:

### Run

| Field | Value |
| --- | --- |
| backend | claude |
| model | claude-haiku-5-5 |
| client version | 2.1.303 (Claude Code) |
| seed | 106 |
| result lines | 447 |
| runs without a baseline partner | 0 |
| reported cost, USD | 0.1478 |

### Non-inferiority per arm

Non-control tasks. Agresti-Min lower one-sided 95% bound; an arm passes when the bound exceeds -2.0 pp and is inferior when the upper one-sided 95% bound is below -2.0 pp. b: only baseline succeeded; c: only the arm succeeded.

| Arm | Pairs | Unchanged | b | c | Baseline success | Arm success | Difference pp | Lower bound pp | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| table-v2 | 210 | 0 | 4 | 3 | 207/210 (98.6%) | 206/210 (98.1%) | -0.5 | -2.7 | not shown |

### Per category

| Category | Arm | Pairs | Baseline success | Arm success | Difference pp | Lower bound pp | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| aggregation | table-v2 | 35 | 34/35 (97.1%) | 34/35 (97.1%) | +0.0 | -7.7 | not shown |
| api_query | table-v2 | 30 | 30/30 (100.0%) | 30/30 (100.0%) | +0.0 | -5.1 | not shown |
| buried_evidence | table-v2 | 10 | 10/10 (100.0%) | 10/10 (100.0%) | +0.0 | -13.7 | not shown |
| code_search | table-v2 | 35 | 35/35 (100.0%) | 35/35 (100.0%) | +0.0 | -4.4 | not shown |
| exact_id | table-v2 | 35 | 33/35 (94.3%) | 34/35 (97.1%) | +2.9 | -6.2 | not shown |
| failure_diagnosis | table-v2 | 35 | 35/35 (100.0%) | 34/35 (97.1%) | -2.9 | -8.9 | not shown |
| issue_triage | table-v2 | 30 | 30/30 (100.0%) | 29/30 (96.7%) | -3.3 | -10.3 | not shown |

### Table format read-back

Non-control tasks that ran both table arms. Control tasks are excluded.

| Format | Arm | Runs | Success | Wrong | Abstain | Backend error |
| --- | --- | --- | --- | --- | --- | --- |
| table-v1 CSV | table-csv | 0 | n/a | 0 | 0 | 0 |
| Markdown-KV | table-mdkv | 0 | n/a | 0 | 0 | 0 |

### Controls

| Control | Arm | Pairs | Baseline success | Arm success | Unchanged | Discordant | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| evidence_drop | drop-evidence | 6 | 6/6 (100.0%) | 0/6 (0.0%) | 0 | 6 | evidence loss detected |
| evidence_drop | table-v2 | 3 | 3/3 (100.0%) | 3/3 (100.0%) | 0 | 0 | evidence kept |
| incompressible | table-v2 | 6 | 6/6 (100.0%) | 6/6 (100.0%) | 6 | 0 | all variants unchanged |

### Per arm

All tasks, controls included. Payload tokens are estimates (deslag-estimate-v1). The columns from Input tokens to Turns are backend-reported sums; n/a means not reported, and a negative or non-finite value counts as not reported. Wall ms is the summed harness wall clock of the backend calls.

| Arm | Runs | Success | Wrong | Abstain | Backend error | Payload tokens (estimate) | Input tokens | Output tokens | Cache read | Cache creation | Cost USD | Duration ms | Turns | Wall ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| baseline | 222 | 219/222 (98.6%) | 3 | 0 | 0 | 154311 | 446 | 19921 | 1446 | 372152 | 0.0844 | 363479 | 223 | 637909 |
| table-v2 | 219 | 215/219 (98.2%) | 4 | 0 | 0 | 64521 | 438 | 24890 | 3086 | 245870 | 0.0617 | 357941 | 219 | 627784 |
| drop-evidence | 6 | 0/6 (0.0%) | 0 | 6 | 0 | 3259 | 12 | 399 | 1242 | 7115 | 0.0016 | 9582 | 6 | 17077 |

### Paired savings

Secondary measure; it never decides pass or fail. Non-control tasks; drop-evidence is a control arm and has no row. Saved is baseline minus arm, summed over the pairs. The estimate columns sum payload tokens (deslag-estimate-v1) over all pairs. The Claude columns sum backend-reported input, cache read and cache creation tokens over the pairs where both runs report all three; they include the system prompt and client overhead, which cancel in the difference but dilute the percentage. Estimate error % is the estimate saved on those same pairs minus the Claude saved, over the absolute Claude saved; positive means the estimate overstates the saving. A pair whose baseline or arm payload estimate is not a finite number of at least 0 is dropped from the whole row.

| Arm | Pairs | Saved tokens (estimate) | Saved % (estimate) | Pairs with Claude tokens | Baseline prompt tokens (Claude) | Arm prompt tokens (Claude) | Saved tokens (Claude) | Saved % (Claude) | Estimate error % |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| table-v2 | 210 | 87196 | 57.9 | 210 | 360969 | 241890 | 119079 | 33.0 | -26.8 |

## Mock dry run

The mock backend answers with the accepted answer when it occurs verbatim in the payload, else `NOT_FOUND`. It measures evidence presence only. Its results show that the harness pairs runs, scores them, detects evidence loss and keeps the incompressible controls unchanged. The run covers the full corpus (228 tasks, 935 runs). The output was reproduced on the corpus generated with seed 7700 at commit e159c9d (`tests/evaluations/generated/manifest.json` sha256 `e620eeec695f7a909b58eb4e087d22e21241286f2681c5b824a820588171d848`). The mock reports no usage, so the Claude columns of the paired savings table show `n/a`. The verdicts below describe the mock backend only.

```sh
mkdir -p tmp/eval
rm -f tmp/eval/mock-seed16.jsonl
cargo run -q -p deslag-eval --locked -- run --manifest tests/evaluations/manifest.json \
  --backend mock --seed 16 --out tmp/eval/mock-seed16.jsonl
cargo run -q -p deslag-eval --locked -- report tmp/eval/mock-seed16.jsonl
```

Report output, verbatim:

### Run

| Field | Value |
| --- | --- |
| backend | mock |
| model | n/a |
| client version | deslag-eval 0.1.0-alpha.2 |
| seed | 16 |
| result lines | 935 |
| runs without a baseline partner | 0 |
| reported cost, USD | n/a |

### Non-inferiority per arm

Non-control tasks. Agresti-Min lower one-sided 95% bound; an arm passes when the bound exceeds -2.0 pp and is inferior when the upper one-sided 95% bound is below -2.0 pp. b: only baseline succeeded; c: only the arm succeeded.

| Arm | Pairs | Unchanged | b | c | Baseline success | Arm success | Difference pp | Lower bound pp | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| json-minify | 106 | 0 | 0 | 0 | 106/106 (100.0%) | 106/106 (100.0%) | +0.0 | -1.5 | passes |
| lines-v1 | 106 | 2 | 0 | 0 | 106/106 (100.0%) | 106/106 (100.0%) | +0.0 | -1.5 | passes |
| lines-v2 | 105 | 0 | 0 | 0 | 105/105 (100.0%) | 105/105 (100.0%) | +0.0 | -1.5 | passes |
| table-csv | 105 | 1 | 0 | 0 | 105/105 (100.0%) | 105/105 (100.0%) | +0.0 | -1.5 | passes |
| table-mdkv | 105 | 1 | 0 | 0 | 105/105 (100.0%) | 105/105 (100.0%) | +0.0 | -1.5 | passes |
| table-v2 | 105 | 0 | 0 | 0 | 105/105 (100.0%) | 105/105 (100.0%) | +0.0 | -1.5 | passes |

### Per category

| Category | Arm | Pairs | Baseline success | Arm success | Difference pp | Lower bound pp | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| aggregation | json-minify | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| aggregation | table-csv | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| aggregation | table-mdkv | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| aggregation | table-v2 | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| api_query | json-minify | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| api_query | table-csv | 16 | 16/16 (100.0%) | 16/16 (100.0%) | +0.0 | -9.1 | not shown |
| api_query | table-mdkv | 16 | 16/16 (100.0%) | 16/16 (100.0%) | +0.0 | -9.1 | not shown |
| api_query | table-v2 | 16 | 16/16 (100.0%) | 16/16 (100.0%) | +0.0 | -9.1 | not shown |
| buried_evidence | json-minify | 6 | 6/6 (100.0%) | 6/6 (100.0%) | +0.0 | -20.6 | not shown |
| buried_evidence | lines-v1 | 7 | 7/7 (100.0%) | 7/7 (100.0%) | +0.0 | -18.3 | not shown |
| buried_evidence | lines-v2 | 7 | 7/7 (100.0%) | 7/7 (100.0%) | +0.0 | -18.3 | not shown |
| buried_evidence | table-csv | 6 | 6/6 (100.0%) | 6/6 (100.0%) | +0.0 | -20.6 | not shown |
| buried_evidence | table-mdkv | 6 | 6/6 (100.0%) | 6/6 (100.0%) | +0.0 | -20.6 | not shown |
| buried_evidence | table-v2 | 6 | 6/6 (100.0%) | 6/6 (100.0%) | +0.0 | -20.6 | not shown |
| code_search | json-minify | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| code_search | lines-v1 | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| code_search | table-csv | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| code_search | table-mdkv | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| code_search | table-v2 | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| exact_id | json-minify | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| exact_id | table-csv | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| exact_id | table-mdkv | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| exact_id | table-v2 | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| failure_diagnosis | json-minify | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| failure_diagnosis | lines-v1 | 32 | 32/32 (100.0%) | 32/32 (100.0%) | +0.0 | -4.8 | not shown |
| failure_diagnosis | lines-v2 | 32 | 32/32 (100.0%) | 32/32 (100.0%) | +0.0 | -4.8 | not shown |
| failure_diagnosis | table-csv | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| failure_diagnosis | table-mdkv | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| failure_diagnosis | table-v2 | 17 | 17/17 (100.0%) | 17/17 (100.0%) | +0.0 | -8.7 | not shown |
| issue_triage | json-minify | 15 | 15/15 (100.0%) | 15/15 (100.0%) | +0.0 | -9.7 | not shown |
| issue_triage | table-csv | 15 | 15/15 (100.0%) | 15/15 (100.0%) | +0.0 | -9.7 | not shown |
| issue_triage | table-mdkv | 15 | 15/15 (100.0%) | 15/15 (100.0%) | +0.0 | -9.7 | not shown |
| issue_triage | table-v2 | 15 | 15/15 (100.0%) | 15/15 (100.0%) | +0.0 | -9.7 | not shown |
| logs | lines-v1 | 66 | 66/66 (100.0%) | 66/66 (100.0%) | +0.0 | -2.4 | not shown |
| logs | lines-v2 | 66 | 66/66 (100.0%) | 66/66 (100.0%) | +0.0 | -2.4 | not shown |

### Table format read-back

Non-control tasks that ran both table arms. Control tasks are excluded.

| Format | Arm | Runs | Success | Wrong | Abstain | Backend error |
| --- | --- | --- | --- | --- | --- | --- |
| table-v1 CSV | table-csv | 105 | 105/105 (100.0%) | 0 | 0 | 0 |
| Markdown-KV | table-mdkv | 105 | 105/105 (100.0%) | 0 | 0 | 0 |

### Controls

| Control | Arm | Pairs | Baseline success | Arm success | Unchanged | Discordant | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| evidence_drop | drop-evidence | 8 | 8/8 (100.0%) | 0/8 (0.0%) | 0 | 8 | evidence loss detected |
| evidence_drop | json-minify | 3 | 3/3 (100.0%) | 3/3 (100.0%) | 0 | 0 | evidence kept |
| evidence_drop | lines-v1 | 3 | 3/3 (100.0%) | 3/3 (100.0%) | 3 | 0 | evidence kept |
| evidence_drop | lines-v2 | 4 | 4/4 (100.0%) | 4/4 (100.0%) | 0 | 0 | evidence kept |
| evidence_drop | table-csv | 3 | 3/3 (100.0%) | 3/3 (100.0%) | 0 | 0 | evidence kept |
| evidence_drop | table-mdkv | 3 | 3/3 (100.0%) | 3/3 (100.0%) | 0 | 0 | evidence kept |
| evidence_drop | table-v2 | 3 | 3/3 (100.0%) | 3/3 (100.0%) | 0 | 0 | evidence kept |
| incompressible | json-minify | 8 | 8/8 (100.0%) | 8/8 (100.0%) | 8 | 0 | all variants unchanged |
| incompressible | lines-v1 | 8 | 8/8 (100.0%) | 8/8 (100.0%) | 8 | 0 | all variants unchanged |
| incompressible | lines-v2 | 8 | 8/8 (100.0%) | 8/8 (100.0%) | 8 | 0 | all variants unchanged |
| incompressible | table-csv | 8 | 8/8 (100.0%) | 8/8 (100.0%) | 8 | 0 | all variants unchanged |
| incompressible | table-mdkv | 8 | 8/8 (100.0%) | 8/8 (100.0%) | 8 | 0 | all variants unchanged |
| incompressible | table-v2 | 8 | 8/8 (100.0%) | 8/8 (100.0%) | 8 | 0 | all variants unchanged |

### Per arm

All tasks, controls included. Payload tokens are estimates (deslag-estimate-v1). The columns from Input tokens to Turns are backend-reported sums; n/a means not reported, and a negative or non-finite value counts as not reported. Wall ms is the summed harness wall clock of the backend calls.

| Arm | Runs | Success | Wrong | Abstain | Backend error | Payload tokens (estimate) | Input tokens | Output tokens | Cache read | Cache creation | Cost USD | Duration ms | Turns | Wall ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| baseline | 228 | 228/228 (100.0%) | 0 | 0 | 0 | 190626 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |
| json-minify | 117 | 117/117 (100.0%) | 0 | 0 | 0 | 73653 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |
| table-csv | 116 | 116/116 (100.0%) | 0 | 0 | 0 | 47094 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |
| table-mdkv | 116 | 116/116 (100.0%) | 0 | 0 | 0 | 76653 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |
| table-v2 | 116 | 116/116 (100.0%) | 0 | 0 | 0 | 45913 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |
| lines-v1 | 117 | 117/117 (100.0%) | 0 | 0 | 0 | 78242 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |
| lines-v2 | 117 | 117/117 (100.0%) | 0 | 0 | 0 | 67536 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |
| drop-evidence | 8 | 0/8 (0.0%) | 0 | 8 | 0 | 4506 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |

### Paired savings

Secondary measure; it never decides pass or fail. Non-control tasks; drop-evidence is a control arm and has no row. Saved is baseline minus arm, summed over the pairs. The estimate columns sum payload tokens (deslag-estimate-v1) over all pairs. The Claude columns sum backend-reported input, cache read and cache creation tokens over the pairs where both runs report all three; they include the system prompt and client overhead, which cancel in the difference but dilute the percentage. Estimate error % is the estimate saved on those same pairs minus the Claude saved, over the absolute Claude saved; positive means the estimate overstates the saving. A pair whose baseline or arm payload estimate is not a finite number of at least 0 is dropped from the whole row.

| Arm | Pairs | Saved tokens (estimate) | Saved % (estimate) | Pairs with Claude tokens | Baseline prompt tokens (Claude) | Arm prompt tokens (Claude) | Saved tokens (Claude) | Saved % (Claude) | Estimate error % |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| json-minify | 106 | 31643 | 30.5 | 0 | n/a | n/a | n/a | n/a | n/a |
| lines-v1 | 106 | 5552 | 6.8 | 0 | n/a | n/a | n/a | n/a | n/a |
| lines-v2 | 105 | 15744 | 19.4 | 0 | n/a | n/a | n/a | n/a | n/a |
| table-csv | 105 | 57472 | 55.5 | 0 | n/a | n/a | n/a | n/a | n/a |
| table-mdkv | 105 | 28525 | 27.6 | 0 | n/a | n/a | n/a | n/a | n/a |
| table-v2 | 105 | 58653 | 56.7 | 0 | n/a | n/a | n/a | n/a | n/a |

## Mock dry run (run-3 corpus)

This is the mock dry run over the run-3 corpus (`tests/evaluations/run3/manifest.json`, 222 tasks, 447 runs). The mock backend works as in [Mock dry run](#mock-dry-run): it measures evidence presence only. The run shows that every non-control `table-v2` run keeps the answer, that `drop-evidence` loses the evidence on all 6 evidence-drop controls and that `table-v2` leaves all 6 incompressible controls unchanged. The verdicts below describe the mock backend only. They predict nothing about live run 3.

```sh
mkdir -p tmp/eval
rm -f tmp/eval/mock-run3-seed106.jsonl
cargo run -q -p deslag-eval --locked -- run --manifest tests/evaluations/run3/manifest.json \
  --backend mock --seed 106 --out tmp/eval/mock-run3-seed106.jsonl
cargo run -q -p deslag-eval --locked -- report tmp/eval/mock-run3-seed106.jsonl
```

Report output, verbatim:

### Run

| Field | Value |
| --- | --- |
| backend | mock |
| model | n/a |
| client version | deslag-eval 0.1.0-alpha.4 |
| seed | 106 |
| result lines | 447 |
| runs without a baseline partner | 0 |
| reported cost, USD | n/a |

### Non-inferiority per arm

Non-control tasks. Agresti-Min lower one-sided 95% bound; an arm passes when the bound exceeds -2.0 pp and is inferior when the upper one-sided 95% bound is below -2.0 pp. b: only baseline succeeded; c: only the arm succeeded.

| Arm | Pairs | Unchanged | b | c | Baseline success | Arm success | Difference pp | Lower bound pp | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| table-v2 | 210 | 0 | 0 | 0 | 210/210 (100.0%) | 210/210 (100.0%) | +0.0 | -0.8 | passes |

### Per category

| Category | Arm | Pairs | Baseline success | Arm success | Difference pp | Lower bound pp | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| aggregation | table-v2 | 35 | 35/35 (100.0%) | 35/35 (100.0%) | +0.0 | -4.4 | not shown |
| api_query | table-v2 | 30 | 30/30 (100.0%) | 30/30 (100.0%) | +0.0 | -5.1 | not shown |
| buried_evidence | table-v2 | 10 | 10/10 (100.0%) | 10/10 (100.0%) | +0.0 | -13.7 | not shown |
| code_search | table-v2 | 35 | 35/35 (100.0%) | 35/35 (100.0%) | +0.0 | -4.4 | not shown |
| exact_id | table-v2 | 35 | 35/35 (100.0%) | 35/35 (100.0%) | +0.0 | -4.4 | not shown |
| failure_diagnosis | table-v2 | 35 | 35/35 (100.0%) | 35/35 (100.0%) | +0.0 | -4.4 | not shown |
| issue_triage | table-v2 | 30 | 30/30 (100.0%) | 30/30 (100.0%) | +0.0 | -5.1 | not shown |

### Table format read-back

Non-control tasks that ran both table arms. Control tasks are excluded.

| Format | Arm | Runs | Success | Wrong | Abstain | Backend error |
| --- | --- | --- | --- | --- | --- | --- |
| table-v1 CSV | table-csv | 0 | n/a | 0 | 0 | 0 |
| Markdown-KV | table-mdkv | 0 | n/a | 0 | 0 | 0 |

### Controls

| Control | Arm | Pairs | Baseline success | Arm success | Unchanged | Discordant | Result |
| --- | --- | --- | --- | --- | --- | --- | --- |
| evidence_drop | drop-evidence | 6 | 6/6 (100.0%) | 0/6 (0.0%) | 0 | 6 | evidence loss detected |
| evidence_drop | table-v2 | 3 | 3/3 (100.0%) | 3/3 (100.0%) | 0 | 0 | evidence kept |
| incompressible | table-v2 | 6 | 6/6 (100.0%) | 6/6 (100.0%) | 6 | 0 | all variants unchanged |

### Per arm

All tasks, controls included. Payload tokens are estimates (deslag-estimate-v1). The columns from Input tokens to Turns are backend-reported sums; n/a means not reported, and a negative or non-finite value counts as not reported. Wall ms is the summed harness wall clock of the backend calls.

| Arm | Runs | Success | Wrong | Abstain | Backend error | Payload tokens (estimate) | Input tokens | Output tokens | Cache read | Cache creation | Cost USD | Duration ms | Turns | Wall ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| baseline | 222 | 222/222 (100.0%) | 0 | 0 | 0 | 154311 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |
| table-v2 | 219 | 219/219 (100.0%) | 0 | 0 | 0 | 64521 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |
| drop-evidence | 6 | 0/6 (0.0%) | 0 | 6 | 0 | 3259 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |

### Paired savings

Secondary measure; it never decides pass or fail. Non-control tasks; drop-evidence is a control arm and has no row. Saved is baseline minus arm, summed over the pairs. The estimate columns sum payload tokens (deslag-estimate-v1) over all pairs. The Claude columns sum backend-reported input, cache read and cache creation tokens over the pairs where both runs report all three; they include the system prompt and client overhead, which cancel in the difference but dilute the percentage. Estimate error % is the estimate saved on those same pairs minus the Claude saved, over the absolute Claude saved; positive means the estimate overstates the saving. A pair whose baseline or arm payload estimate is not a finite number of at least 0 is dropped from the whole row.

| Arm | Pairs | Saved tokens (estimate) | Saved % (estimate) | Pairs with Claude tokens | Baseline prompt tokens (Claude) | Arm prompt tokens (Claude) | Saved tokens (Claude) | Saved % (Claude) | Estimate error % |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| table-v2 | 210 | 87196 | 57.9 | 0 | n/a | n/a | n/a | n/a | n/a |

## Payload sizes

Per-arm summary for the full corpus (the pilot and the generated tasks), from `cargo run -q -p deslag-eval --locked -- variants tests/evaluations/manifest.json`. Tokens are `deslag-estimate-v1` estimates. Tasks is the number of tasks that list the arm, controls included. Changed is the number of those tasks where the transform changed the payload. Baseline tokens sum the baseline payload of the same tasks. Arm tokens sum the arm payload.

| Arm | Tasks | Changed | Baseline tokens (estimate) | Arm tokens (estimate) |
| --- | --- | --- | --- | --- |
| json-minify | 117 | 109 | 105991 | 73653 |
| table-csv | 116 | 107 | 105813 | 47094 |
| table-mdkv | 116 | 107 | 105813 | 76653 |
| table-v2 | 116 | 108 | 105813 | 45913 |
| lines-v1 | 117 | 104 | 83794 | 78242 |
| lines-v2 | 117 | 109 | 83523 | 67536 |
| drop-evidence | 8 | 8 | 4665 | 4506 |

The same command prints one row per (task, arm), 935 rows, with bytes, estimated tokens and the changed flag. Add `> tmp/eval/variants.md` to keep the rows.

### Run-3 payload sizes

Per-arm summary for the run-3 corpus, from `cargo run -q -p deslag-eval --locked -- variants tests/evaluations/run3/manifest.json`. The columns are those of the full-corpus table above. `table-v2` lists 219 tasks: 210 non-control tasks, all changed, and 9 controls, of which the 3 JSON evidence-drop controls change.

| Arm | Tasks | Changed | Baseline tokens (estimate) | Arm tokens (estimate) |
| --- | --- | --- | --- | --- |
| table-v2 | 219 | 213 | 153021 | 64521 |
| drop-evidence | 6 | 6 | 3379 | 3259 |

The same command prints one row per (task, arm), 447 rows.

### Pilot payload sizes

These are the rows of the 21 pilot tasks. They are unchanged from before the generated corpus was added.

| Task | Arm | Bytes | Estimated tokens | Changed |
| --- | --- | --- | --- | --- |
| api-users-email | baseline | 7236 | 2460 | no |
| api-users-email | json-minify | 5434 | 1696 | yes |
| api-users-email | table-csv | 2565 | 975 | yes |
| api-users-email | table-mdkv | 5365 | 1735 | yes |
| api-users-email | table-v2 | 2565 | 975 | yes |
| api-repo-branch | baseline | 487 | 178 | no |
| api-repo-branch | json-minify | 399 | 130 | yes |
| api-search-email | baseline | 6627 | 2407 | no |
| api-search-email | json-minify | 4602 | 1754 | yes |
| api-search-email | table-csv | 6627 | 2407 | no |
| api-search-email | table-mdkv | 6627 | 2407 | no |
| api-search-email | table-v2 | 2632 | 1226 | yes |
| code-defines-fn | baseline | 6890 | 2473 | no |
| code-defines-fn | json-minify | 5388 | 1869 | yes |
| code-defines-fn | table-csv | 3639 | 1417 | yes |
| code-defines-fn | table-mdkv | 5339 | 1867 | yes |
| code-defines-fn | table-v2 | 3639 | 1417 | yes |
| code-const-line | baseline | 3077 | 858 | no |
| code-const-line | lines-v1 | 3077 | 858 | no |
| fail-test-name | baseline | 3710 | 991 | no |
| fail-test-name | lines-v1 | 2349 | 712 | yes |
| fail-test-name | lines-v2 | 1839 | 589 | yes |
| fail-ci-job | baseline | 5028 | 1752 | no |
| fail-ci-job | json-minify | 3766 | 1237 | yes |
| fail-ci-job | table-csv | 1867 | 703 | yes |
| fail-ci-job | table-mdkv | 3727 | 1243 | yes |
| fail-ci-job | table-v2 | 1867 | 703 | yes |
| triage-open-p0 | baseline | 6258 | 2081 | no |
| triage-open-p0 | json-minify | 4576 | 1357 | yes |
| triage-open-p0 | table-csv | 2017 | 790 | yes |
| triage-open-p0 | table-mdkv | 4497 | 1390 | yes |
| triage-open-p0 | table-v2 | 2017 | 790 | yes |
| triage-assignee-set | baseline | 6149 | 2332 | no |
| triage-assignee-set | json-minify | 4707 | 1687 | yes |
| triage-assignee-set | table-csv | 2419 | 1118 | yes |
| triage-assignee-set | table-mdkv | 4579 | 1678 | yes |
| triage-assignee-set | table-v2 | 2419 | 1118 | yes |
| logs-503-request | baseline | 6013 | 2320 | no |
| logs-503-request | lines-v1 | 4724 | 1924 | yes |
| logs-503-request | lines-v2 | 2837 | 1435 | yes |
| logs-403-ip | baseline | 8907 | 4672 | no |
| logs-403-ip | lines-v1 | 8907 | 4672 | no |
| logs-403-ip | lines-v2 | 6375 | 3307 | yes |
| agg-max-errors | baseline | 6195 | 2237 | no |
| agg-max-errors | json-minify | 4393 | 1513 | yes |
| agg-max-errors | table-csv | 1576 | 701 | yes |
| agg-max-errors | table-mdkv | 4336 | 1541 | yes |
| agg-max-errors | table-v2 | 1576 | 701 | yes |
| agg-max-duration | baseline | 5364 | 1915 | no |
| agg-max-duration | json-minify | 3862 | 1311 | yes |
| agg-max-duration | table-csv | 1525 | 639 | yes |
| agg-max-duration | table-mdkv | 3825 | 1339 | yes |
| agg-max-duration | table-v2 | 1525 | 639 | yes |
| id-order-by-amount | baseline | 11347 | 4387 | no |
| id-order-by-amount | json-minify | 8465 | 3102 | yes |
| id-order-by-amount | table-csv | 2948 | 1538 | yes |
| id-order-by-amount | table-mdkv | 8148 | 3058 | yes |
| id-order-by-amount | table-v2 | 2948 | 1538 | yes |
| id-deploy-commit | baseline | 6779 | 2849 | no |
| id-deploy-commit | json-minify | 5577 | 2284 | yes |
| id-deploy-commit | table-csv | 3365 | 1752 | yes |
| id-deploy-commit | table-mdkv | 5405 | 2272 | yes |
| id-deploy-commit | table-v2 | 3365 | 1752 | yes |
| buried-key-delete | baseline | 20100 | 8682 | no |
| buried-key-delete | json-minify | 14698 | 6277 | yes |
| buried-key-delete | table-csv | 7709 | 4505 | yes |
| buried-key-delete | table-mdkv | 14009 | 6155 | yes |
| buried-key-delete | table-v2 | 7709 | 4505 | yes |
| buried-artifact-sha | baseline | 8028 | 3604 | no |
| buried-artifact-sha | lines-v1 | 7734 | 3559 | yes |
| buried-artifact-sha | lines-v2 | 5720 | 3124 | yes |
| control-prose | baseline | 423 | 92 | no |
| control-prose | json-minify | 423 | 92 | no |
| control-prose | table-csv | 423 | 92 | no |
| control-prose | table-mdkv | 423 | 92 | no |
| control-prose | table-v2 | 423 | 92 | no |
| control-prose | lines-v1 | 423 | 92 | no |
| control-prose | lines-v2 | 423 | 92 | no |
| control-minimal-json | baseline | 57 | 17 | no |
| control-minimal-json | json-minify | 57 | 17 | no |
| control-minimal-json | table-csv | 57 | 17 | no |
| control-minimal-json | table-mdkv | 57 | 17 | no |
| control-minimal-json | table-v2 | 57 | 17 | no |
| control-minimal-json | lines-v1 | 57 | 17 | no |
| control-minimal-json | lines-v2 | 57 | 17 | no |
| control-drop-revoker | baseline | 1563 | 588 | no |
| control-drop-revoker | drop-evidence | 1531 | 577 | yes |
| control-drop-alert | baseline | 1275 | 587 | no |
| control-drop-alert | drop-evidence | 1205 | 559 | yes |
| control-drop-alert | lines-v2 | 875 | 515 | yes |
