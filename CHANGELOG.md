# Changelog

All notable changes to deslag are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/). Pending entries live in `changes/`
until a release folds them in; see `changes/README.md` and `docs/release.md`.

## Unreleased

## 0.1.0-alpha.3 - 2026-10-14

### Added

- Every transform applies the context token gain thresholds `min_estimated_token_savings` (default 64) and `min_estimated_savings_permille` (default 100, ratio 0.10) when a token counter is set, and skips with `insufficient_gain` below them (#63)
- `deslag hook claude --minify-json` also minifies JSON in the text blocks of MCP tool results; Bash image results, stderr and all other fields and tools stay unchanged (#17)
- `deslag hook claude --minify-json` leaves the output unchanged and reports on stderr when another `PostToolUse` hook in user, project or enabled-plugin settings may also replace it (#17)
- `deslag local run [--retention-days N]` runs an optional foreground service that aggregates compression reports into hourly buckets in a private `stats.jsonl` (#21)
- `deslag local start [--retention-days N]` starts that service in the background, `deslag local stop` writes the open hour and stops it, and `deslag local status` reports whether it runs (#21)
- The service answers `ping`, `flush` and `stop` with `ok`. When the `stats.jsonl` write fails, `flush` and `stop` log the error to stderr, answer `flush failed: <error>` and keep the service running (#21)
- The drop count of an hour is the sum of `dropped` over all its `stats.jsonl` lines: report lines carry sender delivery failures and service lines carry service drops (#21)
- `deslag_telemetry::ensure_state_dir` follows a symlinked state directory and refuses a non-directory or a directory not owned by the current user, so `deslag local start` and `run` fail in those cases (#21)
- `deslag stats [--json] [--since <n>h|<n>d]` prints the aggregates as a table or as versioned JSON (#21)
- The local service answers `GET /session/<id>` on `local-http.sock` with the `deslag stats --json --session <id>` document of the last 24 hours (#91)
- The local service answers a `requests` control message on `local.sock`. It writes only the queued `requests.jsonl` lines, plus the buckets when one holds drops, and answers `ok`, or `flush failed: <error>` when the write fails (#91)
- The Claude Code usage module has an opt-in savings widget. With `DESLAG_SAVINGS_WIDGET=1`, the status line shows the estimated tokens removed from each model request next to its provider input tokens, plus the session total of the last 24 hours, labeled as an estimate (#91)
- `docs/demo.md` walks through hook compression and the savings widget in a throwaway session (#91)
- The proxy pins one decision per tool result in a local session manifest, replays it on every later request and returns `400` when the replay state cannot reproduce the history (#18)
- With replay, a busy, expired or panicked proxy compression job no longer forwards the whole body unrewritten: the new tool result is recorded as `unchanged`, and a result pinned as json-minify is re-encoded outside the bounded executor, so history bytes never change (#18)
- `deslag_proxy::session` stores the compression owner (`hook`, `proxy` or `none`) in `<state dir>/owner` and keeps per-session replay manifests that pin one decision per tool result, committed atomically and removed once their mtime is older than 30 days (a replay that commits nothing refreshes the mtime at most once a day, so an idle manifest can be removed up to one day before 30 days without use); nothing calls it yet (#18)
- The Claude Code plugin has an opt-in usage module that sends the provider usage of each model request to the local service. Enable it with `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1` and `DESLAG_USAGE_STATS=1`. Failed and dropped sends are counted (#90)
- The local service accepts usage records over the HTTP socket `local-http.sock` and stores one line per model request in `requests.jsonl` (#90)
- Hook compression reports carry the session ID, and the service attributes them to the next model request of that session (#90)
- `deslag stats --json` reports `totals.usage`, and `deslag stats --json --session <id>` lists the requests of one session with provider usage and estimated removed tokens (#90)

### Changed

- `deslag hook claude --minify-json` and the proxy run compression under the bounded executor (4 parallel jobs, 10 ms per job); a busy, expired or panicked job leaves the hook output unchanged with the stderr diagnostic `deslag: compression <busy|budget_exceeded|job_panicked>; tool output unchanged` and forwards the original proxy body; a panicked job also prints the default Rust panic message to stderr (#63)
- The proxy moves request rewriting from the async worker threads to the blocking thread pool. The inspection permit stays held until the rewrite ends, also when the client disconnects; `deslag_proxy::Limits` gains an `executor` field and is no longer `Copy` (#63)
- The `lines-v2` encoder no longer stops templating below back-to-back or periodic outlier lines; it writes the best-paying group prefix and stops a group before a paying run when the run's first row adds a variable index or when the run directive saves more than the run's copies would as group rows. The `line-templates` transform version is now 2 (#59)
- The local service counts a report with a byte size above 2^32 - 1 as a service drop, since no real tool result is that large (#21)
- The local service counts a `would_change` report with a null `candidate_bytes` as a service drop, since deslag-core always sets it in observe mode. A null token candidate is kept, and a token sum that includes one is null (#21)
- Rotation merges `stats.jsonl` lines with equal keys into one line. It removes every line that fails validation: another schema version, a value outside the allowlists, a missing or non-integer field, an hour that is not a multiple of 3600, partly null dimensions, or outcome `would_change` with a null `candidate_bytes`. It removes unknown token labels and keeps the rest of the line. Before, rotation kept any JSON line with a numeric `hour` (#21)
- The release workflow publishes through `scripts/release/publish.sh`, with tests for re-runs on complete, empty and partial releases (#45)
- `deslag stats` reports the `no_data` partial reason only when the window has no lines and the service is not running. A running service with an empty window no longer reports `no_data` (#91)
- The plugin and marketplace descriptions name the hook adapter, the opt-in usage module and the opt-in savings widget, and no longer call the plugin a no-op (#91)
- `--minify-json` on `deslag hook claude` and `deslag proxy` now requires an owner file in the state dir: each compresses only when `<state dir>/owner` names that component. An existing `--minify-json` setup without an owner file stops compressing after upgrade. Change the owner only between sessions: after a mid-session change from `proxy` to `hook`, the proxy keeps replaying earlier decisions while the hook compresses new results (#18)
- `deslag proxy --minify-json` now needs a state directory and fails at startup without one. It rewrites only requests with an `X-Claude-Code-Session-Id` header and removes manifests not modified for 30 days at startup (#18)
- The `table-v2` encoder now picks the table or bucket block for a hole or a sub-block array after sub-tables apply to both candidates, as it already did for a bare array, so every site uses the same selection order. The chosen block can still differ beyond its labels when label length changes a reference-cell decision or the nesting level reaches its limit. Selection compares bytes only, so with a token counter a bare array whose table-v1 text fails only the token check now passes through where a longer bucket block used to win. The `json-table` transform version is now 5 (#65)
- The plugin's `hooks/hooks.json` holds `"modules"` next to `"hooks"`. This mixed file passes `claude plugin validate` and `claude plugin test` on Claude Code 2.1.296 and 2.1.299. A live hook run on 2.1.296 is pending (#90)
- `totals.dropped` in `deslag stats --json` also counts the drops carried by usage records (#90)

### Fixed

- `bump.sh` refreshes `fuzz/Cargo.lock`, and `check-versions.sh` fails when it pins another `deslag-core` version (#67)
- The `json-table` transform encodes a 1 MiB bare array in about 9 to 10 ms instead of about 20 ms at p50 on the benchmark machine. The encoder minifies the input once, builds the table block once for the table-v1 and table-v2 forms, and skips the sub-table rewrite for arrays without stringified JSON. Output bytes are unchanged (#79)

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
- `deslag-core` table-v2 splits a heterogeneous array of objects, such as events whose fields differ by a `type` field, into one table per discriminator value plus an order line, and decodes the rows back in their original order. This applies to bare top-level arrays, which keep table-v1 output unless the bucket form is shorter, and to arrays inside a wrapper object (#43)
- `deslag-core` table-v2 encodes a string cell whose text is minified JSON, such as an array of objects or an object that wraps such arrays, as a sub-table in a `json` column, and decodes it back to the same string lexeme. Sub-tables nest at most 4 levels. The `json-table` transform version is 4, because encoder output changes (#43)
- `deslag-core` adds the `table-v1` encoder and decoder for arrays of JSON objects, with uniform nested objects flattened to dotted columns and eligibility and gain checks. `compress` uses it only when the opt-in `json_table` switch is on, bounded by `Limits::max_table_rows` and falling back to the JSON minifier. An evaluation-only Markdown-KV formatter renders the same table. The format is specified in `docs/compression-formats.md` (#12)
- `deslag-core` adds the `table-v2` format for arrays of objects inside a wrapper object, such as `{"results":[...],"total":1000}`: the wrapper's other members stay byte-identical in a skeleton line and each eligible member array becomes a table-v1-style block. With `json_table` on, `compress` emits it with format version 2, keeps table-v1 output (format version 1) for bare top-level arrays unless a table-v2 block is shorter, and validates with a decoder that reads both versions (#43)
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
