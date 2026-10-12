# Paired task evaluation

The method, the scoring rules and the non-inferiority criterion are predeclared in [benchmark-methodology.md](../benchmark-methodology.md). The corpus is `tests/evaluations/manifest.json`: 20 synthetic tasks, 16 of them in the PLAN §14.4 categories, 2 incompressible controls and 2 evidence-drop controls.

No live model results exist yet. This file holds the pending live run, a two-call live plumbing smoke check and the offline mock dry run.

## Status

| Field | Value |
| --- | --- |
| Live model results | none, pending (see Pending live runs) |
| Mock dry run | below, plumbing evidence only |
| Corpus size per arm | 4 to 11 non-control pairs; the criterion needs at least 81, so every verdict is `not shown` by construction |
| table-v1 CSV vs Markdown-KV | no model comparison yet; CSV cannot become a default before one exists |
| Generated | 2026-10-12T01:12:13Z, commit efaff80f0843acea3df93db427f5c11a89568ee4 |

## Pending live runs

Run 1: one model, one seed, every (task, arm) pair. The command needs authorized Claude credentials and spends money. CI never runs it.

```sh
mkdir -p tmp/eval
DESLAG_EVAL_LIVE=1 cargo run -q -p deslag-eval --locked -- run \
  --manifest tests/evaluations/manifest.json \
  --backend claude --model claude-haiku-5-5 --seed 16 \
  --budget-usd 3.00 --per-run-usd 0.30 \
  --out tmp/eval/live-claude-haiku-5-5-seed16.jsonl
cargo run -q -p deslag-eval --locked -- report tmp/eval/live-claude-haiku-5-5-seed16.jsonl
```

Estimated cost from `deslag-eval estimate --manifest tests/evaluations/manifest.json` (default assumptions). The estimate rests on estimated tokens, assumed output tokens and conservative assumed prices that may exceed the published price of the model. Runs times `--per-run-usd` is a cap only if every call stays within `--per-run-usd`. Before each call, the runner stops when the spent amount plus the per-run cap of USD 0.30 would exceed `--budget-usd 3.00`. Each call passes `--max-budget-usd 0.30`, but the client checks it only after a turn. One call can therefore exceed its cap, and the total can exceed USD 3.00 by that excess. The runner stops with exit 2 after any call that reports more than USD 0.30 and keeps the partial results file. `run` refuses an existing `--out` path.

| Field | Value |
| --- | --- |
| Runs (task, arm) | 73 |
| Estimated input tokens (deslag-estimate-v1, incl. 2000 overhead per run) | 277840 |
| Assumed output tokens per run | 256 |
| Assumed prices, USD per Mtok | input 3 (x1.25 cache write), output 15 |
| Estimated cost | USD 1.32 |
| Cap if every call stays within --per-run-usd (73 runs at 0.3) | USD 21.90 |

After the run, add its report as a new section above the mock dry run, with the client version and the total spend, and update the Status table.

## Live plumbing smoke check

On 2026-10-12, 2 live calls checked the `claude` backend plumbing: task `api-repo-branch`, arms `baseline` and `json-minify`, model `claude-haiku-5-5`, client 2.1.296, seed 16, `--per-run-usd 0.30`, `--budget-usd 0.60`. Both calls answered correctly. The total reported cost was USD 0.0004. The cache-creation tokens were 918 and 868 against payload estimates of 178 and 130. The per-call overhead was therefore about 740 tokens, below the 2000-token overhead that the estimate assumes. This is plumbing evidence only, and the analysis excludes it.

## Mock dry run

The mock backend answers with the accepted answer when it occurs verbatim in the payload, else `NOT_FOUND`. It measures evidence presence only. Its results show that the harness pairs runs, scores them, detects evidence loss and keeps the incompressible controls unchanged.

```sh
mkdir -p tmp/eval
rm -f tmp/eval/mock-seed16.jsonl
cargo run -q -p deslag-eval --locked -- run --manifest tests/evaluations/manifest.json \
  --backend mock --seed 16 --out tmp/eval/mock-seed16.jsonl
cargo run -q -p deslag-eval --locked -- report tmp/eval/mock-seed16.jsonl
```

### Run

| Field | Value |
| --- | --- |
| backend | mock |
| model | n/a |
| client version | deslag-eval 0.1.0-alpha.1 |
| seed | 16 |
| result lines | 73 |
| runs without a baseline partner | 0 |
| reported cost, USD | n/a |

### Non-inferiority per arm

Non-control tasks. Agresti-Min lower one-sided 95% bound; an arm passes when the bound exceeds -2.0 pp and is inferior when the upper one-sided 95% bound is below -2.0 pp. b: only baseline succeeded; c: only the arm succeeded.

| Arm | Pairs | Unchanged | b | c | Baseline success | Arm success | Difference pp | Lower bound pp | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| json-minify | 11 | 0 | 0 | 0 | 11/11 (100.0%) | 11/11 (100.0%) | +0.0 | -12.7 | not shown |
| lines-v1 | 5 | 2 | 0 | 0 | 5/5 (100.0%) | 5/5 (100.0%) | +0.0 | -23.5 | not shown |
| lines-v2 | 4 | 0 | 0 | 0 | 4/4 (100.0%) | 4/4 (100.0%) | +0.0 | -27.4 | not shown |
| table-csv | 10 | 0 | 0 | 0 | 10/10 (100.0%) | 10/10 (100.0%) | +0.0 | -13.7 | not shown |
| table-mdkv | 10 | 0 | 0 | 0 | 10/10 (100.0%) | 10/10 (100.0%) | +0.0 | -13.7 | not shown |

### Per category

| Category | Arm | Pairs | Baseline success | Arm success | Difference pp | Lower bound pp | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| aggregation | json-minify | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| aggregation | table-csv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| aggregation | table-mdkv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| api_query | json-minify | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| api_query | table-csv | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| api_query | table-mdkv | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| buried_evidence | json-minify | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| buried_evidence | lines-v1 | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| buried_evidence | lines-v2 | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| buried_evidence | table-csv | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| buried_evidence | table-mdkv | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| code_search | json-minify | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| code_search | lines-v1 | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| code_search | table-csv | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| code_search | table-mdkv | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| exact_id | json-minify | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| exact_id | table-csv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| exact_id | table-mdkv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| failure_diagnosis | json-minify | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| failure_diagnosis | lines-v1 | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| failure_diagnosis | lines-v2 | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| failure_diagnosis | table-csv | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| failure_diagnosis | table-mdkv | 1 | 1/1 (100.0%) | 1/1 (100.0%) | +0.0 | -54.8 | not shown |
| issue_triage | json-minify | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| issue_triage | table-csv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| issue_triage | table-mdkv | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| logs | lines-v1 | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |
| logs | lines-v2 | 2 | 2/2 (100.0%) | 2/2 (100.0%) | +0.0 | -41.1 | not shown |

### Table format read-back

Non-control tasks that ran both table arms. Control tasks are excluded.

| Format | Arm | Runs | Success | Wrong | Abstain | Backend error |
| --- | --- | --- | --- | --- | --- | --- |
| table-v1 CSV | table-csv | 10 | 10/10 (100.0%) | 0 | 0 | 0 |
| Markdown-KV | table-mdkv | 10 | 10/10 (100.0%) | 0 | 0 | 0 |

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

### Per arm

All tasks, controls included. Payload tokens are estimates (deslag-estimate-v1). The columns from Input tokens to Turns are backend-reported sums; n/a means not reported. Wall ms is the summed harness wall clock of the backend calls.

| Arm | Runs | Success | Wrong | Abstain | Backend error | Payload tokens (estimate) | Input tokens | Output tokens | Cache read | Cache creation | Cost USD | Duration ms | Turns | Wall ms |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| baseline | 20 | 20/20 (100.0%) | 0 | 0 | 0 | 45075 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |
| json-minify | 13 | 13/13 (100.0%) | 0 | 0 | 0 | 22572 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |
| table-csv | 12 | 12/12 (100.0%) | 0 | 0 | 0 | 14247 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |
| table-mdkv | 12 | 12/12 (100.0%) | 0 | 0 | 0 | 22387 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |
| lines-v1 | 7 | 7/7 (100.0%) | 0 | 0 | 0 | 11834 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |
| lines-v2 | 7 | 7/7 (100.0%) | 0 | 0 | 0 | 9079 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |
| drop-evidence | 2 | 0/2 (0.0%) | 0 | 2 | 0 | 1136 | n/a | n/a | n/a | n/a | n/a | n/a | n/a | 0 |

## Payload sizes

From `deslag-eval variants tests/evaluations/manifest.json`. Bytes and estimated tokens (`deslag-estimate-v1`) of each arm's payload. `Changed: no` means the transform declined and the arm sends the original.

| Task | Arm | Bytes | Estimated tokens | Changed |
| --- | --- | --- | --- | --- |
| api-users-email | baseline | 7236 | 2460 | no |
| api-users-email | json-minify | 5434 | 1696 | yes |
| api-users-email | table-csv | 2565 | 975 | yes |
| api-users-email | table-mdkv | 5365 | 1735 | yes |
| api-repo-branch | baseline | 487 | 178 | no |
| api-repo-branch | json-minify | 399 | 130 | yes |
| code-defines-fn | baseline | 6890 | 2473 | no |
| code-defines-fn | json-minify | 5388 | 1869 | yes |
| code-defines-fn | table-csv | 3639 | 1417 | yes |
| code-defines-fn | table-mdkv | 5339 | 1867 | yes |
| code-const-line | baseline | 3077 | 858 | no |
| code-const-line | lines-v1 | 3077 | 858 | no |
| fail-test-name | baseline | 3710 | 991 | no |
| fail-test-name | lines-v1 | 2349 | 712 | yes |
| fail-test-name | lines-v2 | 1839 | 589 | yes |
| fail-ci-job | baseline | 5028 | 1752 | no |
| fail-ci-job | json-minify | 3766 | 1237 | yes |
| fail-ci-job | table-csv | 1867 | 703 | yes |
| fail-ci-job | table-mdkv | 3727 | 1243 | yes |
| triage-open-p0 | baseline | 6258 | 2081 | no |
| triage-open-p0 | json-minify | 4576 | 1357 | yes |
| triage-open-p0 | table-csv | 2017 | 790 | yes |
| triage-open-p0 | table-mdkv | 4497 | 1390 | yes |
| triage-assignee-set | baseline | 6149 | 2332 | no |
| triage-assignee-set | json-minify | 4707 | 1687 | yes |
| triage-assignee-set | table-csv | 2419 | 1118 | yes |
| triage-assignee-set | table-mdkv | 4579 | 1678 | yes |
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
| agg-max-duration | baseline | 5364 | 1915 | no |
| agg-max-duration | json-minify | 3862 | 1311 | yes |
| agg-max-duration | table-csv | 1525 | 639 | yes |
| agg-max-duration | table-mdkv | 3825 | 1339 | yes |
| id-order-by-amount | baseline | 11347 | 4387 | no |
| id-order-by-amount | json-minify | 8465 | 3102 | yes |
| id-order-by-amount | table-csv | 2948 | 1538 | yes |
| id-order-by-amount | table-mdkv | 8148 | 3058 | yes |
| id-deploy-commit | baseline | 6779 | 2849 | no |
| id-deploy-commit | json-minify | 5577 | 2284 | yes |
| id-deploy-commit | table-csv | 3365 | 1752 | yes |
| id-deploy-commit | table-mdkv | 5405 | 2272 | yes |
| buried-key-delete | baseline | 20100 | 8682 | no |
| buried-key-delete | json-minify | 14698 | 6277 | yes |
| buried-key-delete | table-csv | 7709 | 4505 | yes |
| buried-key-delete | table-mdkv | 14009 | 6155 | yes |
| buried-artifact-sha | baseline | 8028 | 3604 | no |
| buried-artifact-sha | lines-v1 | 7734 | 3559 | yes |
| buried-artifact-sha | lines-v2 | 5720 | 3124 | yes |
| control-prose | baseline | 423 | 92 | no |
| control-prose | json-minify | 423 | 92 | no |
| control-prose | table-csv | 423 | 92 | no |
| control-prose | table-mdkv | 423 | 92 | no |
| control-prose | lines-v1 | 423 | 92 | no |
| control-prose | lines-v2 | 423 | 92 | no |
| control-minimal-json | baseline | 57 | 17 | no |
| control-minimal-json | json-minify | 57 | 17 | no |
| control-minimal-json | table-csv | 57 | 17 | no |
| control-minimal-json | table-mdkv | 57 | 17 | no |
| control-minimal-json | lines-v1 | 57 | 17 | no |
| control-minimal-json | lines-v2 | 57 | 17 | no |
| control-drop-revoker | baseline | 1563 | 588 | no |
| control-drop-revoker | drop-evidence | 1531 | 577 | yes |
| control-drop-alert | baseline | 1275 | 587 | no |
| control-drop-alert | drop-evidence | 1205 | 559 | yes |
| control-drop-alert | lines-v2 | 875 | 515 | yes |
