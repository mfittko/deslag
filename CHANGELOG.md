# Changelog

All notable changes to deslag are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/). Pending entries live in `changes/`
until a release folds them in; see `changes/README.md` and `docs/release.md`.

## Unreleased

## 0.1.0-alpha.2 - 2026-10-12

### Added

- `deslag-core` adds `BoundedExecutor`: a parallel job limit and a processing budget (defaults 4 jobs, 10 ms). A busy, expired or panicked call returns `Rejected` so the caller passes the original through. An expired job keeps its permit until it ends (#15)
- `deslag-core` exposes the finalized `compress` contract: off, observe and safe modes, size and depth limits, bounded skip and failure reasons, and a JSON report that marks unknown token counts as `null`. Hook and proxy output is unchanged (#10)
- A development-only `deslag-eval` harness runs paired read-back tasks over original and compressed tool results, with predeclared scoring and a non-inferiority criterion in `docs/benchmark-methodology.md`, a synthetic corpus with incompressible and evidence-drop controls in `tests/evaluations/`, an offline mock backend for CI, and a `claude` backend that needs `DESLAG_EVAL_LIVE=1`, `--model` and `--budget-usd`. Results and the pending live run are in `docs/benchmarks/evaluation.md` (#16)
- `deslag-core` adds the `lines-v1` exact line-run codec, off in `FIRST_SLICE_JSON_MINIFY_CONTEXT`, that encodes runs of identical consecutive lines in plain-text tool output, with an exact decoder. Input that already starts with the `lines-v1` header is skipped as `unsupported_format` (#13)
- `deslag-core` documents and tests that a duplicate or escaped key anywhere in a JSON document keeps the whole document unchanged; minification output is unchanged (#11)
- `deslag-core` adds the `lines-v2` line-template codec, off in `FIRST_SLICE_JSON_MINIFY_CONTEXT` and enabled by `line_templates`, that encodes runs of consecutive plain-text lines differing only in timestamps, IDs, counts or paths as one template and a value table, never merges lines that differ in a constant token such as an error message, and has an exact decoder that also reads `lines-v1` (#42)
- The repository is a Claude Code plugin marketplace: `/plugin marketplace add mfittko/deslag` lists the `deslag` plugin (#40)
- `/deslag:install` downloads the `deslag` release archive for the plugin version, verifies it against `SHA256SUMS` and installs it into the private plugin data directory; `/deslag:status` and `/deslag:uninstall` report on and remove it (#40)
- `deslag-core` table-v2 splits a heterogeneous array of objects, such as events whose fields differ by a `type` field, into one table per discriminator value plus an order line, and decodes the rows back in their original order. This applies to bare top-level arrays, which keep table-v1 output unless the bucket form is shorter, and to arrays inside a wrapper object. The `json-table` transform version is 3, because encoder output changes (#43)
- `deslag-core` table-v2 encodes a string cell whose text is minified JSON, such as an array of objects or an object that wraps such arrays, as a sub-table in a `json` column, and decodes it back to the same string lexeme. Sub-tables nest at most 4 levels. The `json-table` transform version is 4, because encoder output changes (#43)
- `deslag-core` adds the `table-v1` encoder and decoder for arrays of JSON objects, with uniform nested objects flattened to dotted columns and eligibility and gain checks. `compress` uses it only when the opt-in `json_table` switch is on, bounded by `Limits::max_table_rows` and falling back to the JSON minifier. An evaluation-only Markdown-KV formatter renders the same table. The format is specified in `docs/compression-formats.md` (#12)
- `deslag-core` adds the `table-v2` format for arrays of objects inside a wrapper object, such as `{"results":[...],"total":1000}`: the wrapper's other members stay byte-identical in a skeleton line and each eligible member array becomes a table-v1-style block. With `json_table` on, `compress` emits it with format version 2, keeps table-v1 output for bare top-level arrays (still format version 1), and validates with a decoder that reads both versions (#43)
- `deslag-core` provides `EstimateCounter`, a local token estimator that labels every count `estimate` with tokenizer `deslag-estimate-v1` and calls no counting API. Calibration fixtures record its error against offline reference tokenizers (#14)

### Changed

- The plugin hook runs the binary installed by `/deslag:install` and no longer needs `deslag` on `PATH`; without a matching binary it leaves tool results unchanged (#40)

## 0.1.0-alpha.1 - 2026-10-11

### Added

- Rust workspace with a pinned toolchain and CI that runs fmt, clippy and tests (#27)
- `deslag` Claude Code PostToolUse hook adapter; it is a no-op unless a transform flag is set (#28)
- Transparent streaming reverse proxy for the Anthropic Messages API (#29)
- Proxy connection cap and header-read, connect and pool idle timeouts (#36)
- Decision record for session identity, ownership and replay (#34)
- Opt-in experimental JSON minification of Bash tool output: `deslag hook claude --minify-json` and `deslag proxy --minify-json`, plus benchmark runners and a measured baseline (#37)
- Release pipeline: changelog fragments, version lockstep check, bump script and a tag-driven release workflow that publishes macOS arm64 and Linux x86_64 archives with SHA256SUMS and build provenance (#39)
- `deslag proxy --upstream <url> [--listen <addr>] [--minify-json]` runs the proxy from the `deslag` CLI and reports the bound address (#33)

### Changed

- `deslag proxy` replaces the unreleased `deslag-proxy` binary (#33)

### Fixed

- The proxy keeps request paths inside the configured base path (#35)
- `Upstream` debug output redacts the upstream base path like its display form (#33)
