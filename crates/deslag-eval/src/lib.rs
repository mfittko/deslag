//! Scoring for `deslag-eval`. The rules are predeclared in
//! `docs/benchmark-methodology.md`.

pub mod score;

/// Every arm except `baseline`, which every task runs.
pub const ARMS: [&str; 6] = [
    "json-minify",
    "table-csv",
    "table-mdkv",
    "lines-v1",
    "lines-v2",
    "drop-evidence",
];
