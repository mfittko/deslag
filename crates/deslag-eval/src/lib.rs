//! Scoring and reporting for `deslag-eval`. The rules are predeclared in
//! `docs/benchmark-methodology.md`.

pub mod report;
pub mod score;

/// Every arm except `baseline`, which every task runs.
pub const ARMS: [&str; 7] = [
    "json-minify",
    "table-csv",
    "table-mdkv",
    "table-v2",
    "lines-v1",
    "lines-v2",
    "drop-evidence",
];
