# Fuzz runs

Each target ran for 60 s with `RUSTC_BOOTSTRAP=1 cargo fuzz run <target> -s none -- -max_total_time=60`. Every corpus started empty. Corpus and artifacts are not tracked. The `json_minify`, `json_table` and `exact_line_runs` runs ran concurrently.

| Field | Value |
| --- | --- |
| Date (UTC) | 2026-10-12 |
| Commit | e04ccbd (`test(fuzz): add line-templates target`) for `line_templates` and `compress`; 8f8dbc3 (`docs(fuzz): record line-templates and compress runs`) for the other targets. Both commits contain the lines-v2 merge cf465cb. |
| rustc | rustc 1.96.0 (ac68faa20 2026-05-25) (Homebrew), stable with `RUSTC_BOOTSTRAP=1` |
| cargo-fuzz | 0.13.2 |
| libFuzzer binding | libfuzzer-sys 0.4.13 |
| Machine | Apple M5 Max, 18 logical CPUs |

| Target | Runs | Coverage (cov / ft) | Corpus | Crashes |
| --- | --- | --- | --- | --- |
| json_minify | 40,860,161 | 301 / 1456 | 679 inputs, 30 KB | 0 |
| json_table | 20,611,576 | 964 / 4857 | 1297 inputs, 613 KB | 0 |
| exact_line_runs | 9,528,225 | 106 / 550 | 154 inputs, 33 KB | 0 |
| line_templates | 2,443,496 | 324 / 2239 | 524 inputs, 186 KB | 0 |
| compress | 3,012,610 | 699 / 3025 | 930 inputs, 289 KB | 0 |
