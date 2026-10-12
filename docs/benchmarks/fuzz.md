# Fuzz runs

Each target ran for 60 s with `RUSTC_BOOTSTRAP=1 cargo fuzz run <target> -s none -- -max_total_time=60`. Every corpus started empty. Corpus and artifacts are not tracked.

| Field | Value |
| --- | --- |
| Date (UTC) | 2026-10-12 |
| Commit | 12278db (`test(fuzz): check outputs against a reference minifier and decoders`) |
| rustc | rustc 1.96.0 (ac68faa20 2026-05-25) (Homebrew), stable with `RUSTC_BOOTSTRAP=1` |
| cargo-fuzz | 0.13.2 |
| libFuzzer binding | libfuzzer-sys 0.4.13 |
| Machine | Apple M5 Max, 18 logical CPUs |

| Target | Runs | Coverage (cov / ft) | Corpus | Crashes |
| --- | --- | --- | --- | --- |
| json_minify | 42,097,458 | 303 / 1520 | 714 inputs, 70 KB | 0 |
| json_table | 23,403,444 | 957 / 4999 | 1368 inputs, 406 KB | 0 |
| exact_line_runs | 12,152,517 | 98 / 495 | 150 inputs, 32 KB | 0 |
| compress | 5,087,139 | 888 / 3379 | 1033 inputs, 732 KB | 0 |
