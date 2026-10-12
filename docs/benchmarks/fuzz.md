# Fuzz runs

Each target ran for 60 s with `RUSTC_BOOTSTRAP=1 cargo fuzz run <target> -s none -- -max_total_time=60`. Corpus and artifacts are not tracked.

| Field | Value |
| --- | --- |
| Date (UTC) | 2026-10-12 |
| Commit | b31d509 (`test(fuzz): add cargo-fuzz targets for each transform`) |
| rustc | rustc 1.96.0 (ac68faa20 2026-05-25) (Homebrew), stable with `RUSTC_BOOTSTRAP=1` |
| cargo-fuzz | 0.13.2 |
| libFuzzer binding | libfuzzer-sys 0.4.13 |
| Machine | Apple Silicon, macOS (Darwin 25.6.0) |

| Target | Runs | Coverage (cov / ft) | Corpus | Crashes |
| --- | --- | --- | --- | --- |
| json_minify | 38,804,649 | 265 / 1265 | 617 inputs, 54 KB | 0 |
| json_table | 19,862,883 | 981 / 4817 | 1267 inputs, 472 KB | 0 |
| exact_line_runs | 11,558,730 | 98 / 493 | 160 inputs, 37 KB | 0 |
| compress | 8,158,891 | 1032 / 3904 | 1022 inputs, 483 KB | 0 |
