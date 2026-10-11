# Token estimate calibration fixtures

These fixtures calibrate `EstimateCounter` (`deslag-estimate-v1`) in
`crates/deslag-core/src/tokens.rs`. The test `crates/deslag-core/tests/tokens.rs`
enforces the bounds below.

## Corpus

Every sample file was written for this repository. No file is copied
from a third-party source.

| File | Content |
| --- | --- |
| `pretty.json` | Indented configuration-style JSON object |
| `pretty.min.json` | `pretty.json` minified |
| `records.json` | Indented array of repeated records with the same keys |
| `records.min.json` | `records.json` minified |
| `log.txt` | Application log excerpt |
| `source_code.txt` | Rust source code |
| `prose.txt` | English prose |
| `cjk.txt` | Japanese, Korean, Chinese, accented Latin and emoji |

The `*.min.json` files are generated. The test checks that they equal the
output of `deslag_core::minify_json` for the pretty file.

## Reference counts

`reference.tsv` holds the reference counts. The generator is
`generate_reference.py`.

- Tool: `tiktoken` 0.14.0, Python 3.14.7.
- Encodings: `o200k_base` and `cl100k_base`.
- Date: 2026-10-11.
- Command, in a throwaway virtualenv outside the repository:

  ```sh
  pip install tiktoken==0.14.0
  python -I generate_reference.py crates/deslag-core/tests/fixtures/tokens
  ```

These encodings belong to other model families. Their counts are calibration
references. They are not Claude counts, and no deslag count is exact for any
model family. Calibration against live Claude counts is pending: it needs an
authorized offline run of a provider token-counting endpoint, and no API key
was available. Production code never calls a counting API.

## Measured error

Relative error is `(estimate - reference) / reference`.

| File | Estimate | o200k_base | Error | cl100k_base | Error |
| --- | ---: | ---: | ---: | ---: | ---: |
| `pretty.json` | 217 | 212 | +2.4% | 210 | +3.3% |
| `pretty.min.json` | 157 | 140 | +12.1% | 136 | +15.4% |
| `records.json` | 429 | 435 | -1.4% | 436 | -1.6% |
| `records.min.json` | 281 | 268 | +4.9% | 269 | +4.5% |
| `log.txt` | 396 | 387 | +2.3% | 387 | +2.3% |
| `source_code.txt` | 196 | 175 | +12.0% | 171 | +14.6% |
| `prose.txt` | 120 | 114 | +5.3% | 114 | +5.3% |
| `cjk.txt` | 148 | 110 | +34.5% | 158 | -6.3% |
| Aggregate | 1944 | 1841 | +5.6% | 1881 | +3.3% |

Minification savings ratio, `(pretty - minified) / pretty`:

| Pair | Estimate | o200k_base | cl100k_base |
| --- | ---: | ---: | ---: |
| `pretty.json` | 0.276 | 0.340 | 0.352 |
| `records.json` | 0.345 | 0.384 | 0.383 |

## Recorded bounds and bias

- Per-fixture error: at most 40% in either direction. Non-ASCII text dominates
  this bound because the two references differ by 44% on `cjk.txt`.
- Aggregate error per encoding: at most 10%.
- Savings ratio: the estimate never exceeds the reference and undershoots it
  by at most 0.10.

The estimator tends to overestimate tokens, most for minified JSON. It
therefore underestimates savings from whitespace removal. This bias is
deliberate: a gain decision based on the estimate does not overstate the gain.

A heuristic change that moves any count must bump the label version, then
update this table and the bounds in the test.
