# Token estimate calibration fixtures

These fixtures calibrate `EstimateCounter` (`deslag-estimate-v1`) in
`crates/deslag-core/src/tokens.rs`. The test `crates/deslag-core/tests/tokens.rs`
enforces the bounds below and pins the estimate for every fixture.

## Corpus

Every sample file was written for this repository.

| File | Content |
| --- | --- |
| `pretty.json` | Indented configuration-style JSON object |
| `pretty.min.json` | `pretty.json` minified |
| `records.json` | Indented array of repeated records with the same keys |
| `records.min.json` | `records.json` minified |
| `nested.json` | 30 copies of `{"a": {}, "b": []}`, indented with 2 spaces |
| `nested.min.json` | `nested.json` minified |
| `nested_tab.json` | The `nested.json` value, indented with tabs |
| `nested_tab.min.json` | `nested_tab.json` minified |
| `log.txt` | Application log excerpt |
| `source_code.txt` | Rust source code |
| `prose.txt` | English prose |
| `cjk.txt` | Japanese, Korean, Chinese, accented Latin and emoji |

The `nested` pair is punctuation-dense. After minification, runs such as
`":[]},{"` remain, and the reference tokenizers split them into several
tokens.

The `*.min.json` files are generated. The test checks that they equal the
output of `deslag_core::minify_json` for the pretty file.

## Reference counts

`reference.tsv` holds the reference counts. The generator is
`generate_reference.py`.

- Tool: `tiktoken` 0.14.0, Python 3.14.7.
- Encodings: `o200k_base` and `cl100k_base`.
- Date: 2026-10-11 (UTC).
- Command, from the repository root, in a throwaway virtualenv outside the
  repository:

  ```sh
  pip install tiktoken==0.14.0
  python -I crates/deslag-core/tests/fixtures/tokens/generate_reference.py crates/deslag-core/tests/fixtures/tokens
  ```

On its first run, `tiktoken` downloads the BPE files for both encodings.
Generation calls no provider API, but it needs network access unless the
BPE files are already cached.

These encodings belong to other model families. Their counts are calibration
references. They are not Claude counts, and no deslag count is exact for any
model family. Per-fixture calibration against live Claude counts is pending:
it needs an authorized out-of-band run of a provider token-counting endpoint,
outside production code, and no API key was available. Production code never
calls a counting API. [Error against live Claude savings](#error-against-live-claude-savings)
compares savings, not per-fixture counts.

## Measured error

Relative error is `(estimate - reference) / reference`.

| File | Estimate | o200k_base | Error | cl100k_base | Error |
| --- | ---: | ---: | ---: | ---: | ---: |
| `pretty.json` | 217 | 212 | +2.4% | 210 | +3.3% |
| `pretty.min.json` | 158 | 140 | +12.9% | 136 | +16.2% |
| `records.json` | 429 | 435 | -1.4% | 436 | -1.6% |
| `records.min.json` | 288 | 268 | +7.5% | 269 | +7.1% |
| `nested.json` | 424 | 422 | +0.5% | 422 | +0.5% |
| `nested.min.json` | 270 | 212 | +27.4% | 242 | +11.6% |
| `nested_tab.json` | 424 | 452 | -6.2% | 452 | -6.2% |
| `nested_tab.min.json` | 270 | 212 | +27.4% | 242 | +11.6% |
| `log.txt` | 396 | 387 | +2.3% | 387 | +2.3% |
| `source_code.txt` | 197 | 175 | +12.6% | 171 | +15.2% |
| `prose.txt` | 120 | 114 | +5.3% | 114 | +5.3% |
| `cjk.txt` | 149 | 110 | +35.5% | 158 | -5.7% |
| Aggregate | 3342 | 3139 | +6.5% | 3239 | +3.2% |

Minification savings ratio, `(pretty - minified) / pretty`:

| Pair | Estimate | o200k_base | cl100k_base |
| --- | ---: | ---: | ---: |
| `pretty.json` | 0.272 | 0.340 | 0.352 |
| `records.json` | 0.329 | 0.384 | 0.383 |
| `nested.json` | 0.363 | 0.498 | 0.427 |
| `nested_tab.json` | 0.363 | 0.531 | 0.465 |

## Recorded bounds and bias

- Per-fixture error, every fixture except `cjk.txt`: at most 28% in either
  direction. `nested.min.json` against `o200k_base` sets this bound. The
  references differ by 14% on that file. The estimate must be high enough to
  keep the `nested.json` savings ratio below the `cl100k_base` ratio.
- Per-fixture error, `cjk.txt`: at most 40% in either direction. The two
  references differ by 44% on this file.
- Aggregate error per encoding: at most 10%.
- Savings ratio: the estimate never exceeds the reference and undershoots it
  by at most 0.17. `nested_tab.json` against `o200k_base` sets this bound.
  The estimator gives the space and tab variants the same count, so both
  estimate 0.363. That value must stay below 0.427 (`nested.json`,
  `cl100k_base`), and `o200k_base` gives 0.531 on the tab variant.

The estimator tends to overestimate tokens, most for minified JSON. It
therefore underestimates savings from whitespace removal. This bias is
deliberate: a gain decision based on the estimate does not overstate the gain.

Characters outside the Basic Multilingual Plane, such as emoji, cost two
tokens each. Other non-ASCII, non-whitespace characters cost one token each.
Whitespace of any script, such as U+00A0 or U+3000, follows the whitespace-run
rule. Five U+1F600 characters estimate 10 tokens, and `cl100k_base` gives 10. The ZWJ family
U+1F468 U+200D U+1F469 U+200D U+1F467 estimates 8 tokens. The references give
8 (`o200k_base`) and 13 (`cl100k_base`).

The test pins the estimate for every fixture. A heuristic change that moves
any count must bump the label version, then update these tables, the pinned
counts and the bounds in the test.

## Error against live Claude savings

Live run 2 in [docs/benchmarks/evaluation.md](../../../../../docs/benchmarks/evaluation.md#live-run-2)
(`claude-haiku-5-5`, Claude Code 2.1.303, seed 77, commit 85b9eed) reports
both savings per arm over the same non-control pairs. Estimate saved sums the
`deslag-estimate-v1` payload saving, baseline minus arm. Claude saved sums the
backend-reported `input_tokens + cache_read_tokens + cache_creation_tokens`,
baseline minus arm. Error is `(estimate saved - Claude saved) / |Claude saved|`.

| Arm | Pairs | Estimate saved | Claude saved | Error |
| --- | ---: | ---: | ---: | ---: |
| `json-minify` | 106 | 31643 | 37373 | -15.3% |
| `lines-v1` | 106 | 5552 | 7797 | -28.8% |
| `lines-v2` | 105 | 15744 | 23743 | -33.7% |
| `table-csv` | 105 | 57472 | 77353 | -25.7% |
| `table-mdkv` | 105 | 28525 | 34271 | -16.8% |
| `table-v2` | 105 | 58653 | 78854 | -25.6% |

The estimate understates the Claude saving for every arm, by 15.3% to 33.7%.
This matches the bias in [Recorded bounds and bias](#recorded-bounds-and-bias):
a gain decision based on the estimate does not overstate the gain. One run on
one model gives these numbers. The estimator, its label and its bounds stay
unchanged.
