# deslag

A small, native, deterministic context-compression layer for AI agents.

deslag reduces the representation overhead of tool results before a model sees them. It does not change executed tool calls, it does not silently drop evidence, and telemetry stays off the request path.

> **Status:** early development. The M0 Claude Code plugin and a no-op `PostToolUse` hook adapter (`deslag hook claude`) exist. A first JSON-minification slice is available behind an opt-in `--minify-json` flag on `deslag hook claude` and `deslag proxy`; the hook applies it to Bash stdout and MCP text results, the proxy to Bash and MCP results that are not errors. The hook flag has run against a live client; the proxy flag has not. One configured owner selects whether the hook or the proxy compresses, and the proxy pins its per-result decisions in local session manifests. The [implementation plan](docs/PLAN.md) is the product brief and backlog. Progress is tracked in the [v0.1 epic](https://github.com/mfittko/deslag/issues/1).

## Planned v0.1

- A Rust core (`deslag-core`) with deterministic transforms for model-visible tool results:
  - JSON minification that preserves every value and lexeme (default on).
  - `table-v1` repeated-record encoding for homogeneous JSON arrays (opt-in).
  - `table-v2` encoding for such arrays inside a wrapper object, and for heterogeneous arrays, bare (`table-v2 @0`) or wrapped, split into one table per discriminator value, and for string cells that hold minified JSON, encoded as nested sub-tables up to 4 levels (opt-in).
  - Exact consecutive-line run encoding for plain text (opt-in).
- A Claude Code plugin that runs the native binary as a `PostToolUse` hook.
- A minimal transparent Anthropic Messages proxy with streaming support.
- Local compression statistics that separate observed proposals from emitted reductions.

Structural encodings stay opt-in until paired task evaluations pass. See [PLAN §14.4](docs/PLAN.md#144-agent-effectiveness).

## Proxy limits

`deslag proxy [--upstream <url>] [--listen <addr>] [--minify-json]` runs the proxy. A missing `--upstream` or `--listen` takes its value from the [config file](#config-launcher-status-doctor-and-disable), and a flag wins over the file. It listens on `127.0.0.1:8789` by default. `--listen` takes a literal `IP:port`, for example `127.0.0.1:8789` or `[::1]:8789`, and the IP must be a loopback address. It bounds connections and waiting time. The defaults are constants in `crates/deslag-proxy/src/lib.rs`:

| Limit | Default | Effect |
| --- | --- | --- |
| `MAX_CONNECTIONS` | 128 | Further client connections wait in the kernel listen backlog until a slot frees. The backlog size is a separate listener setting, capped by the OS `somaxconn`. |
| `HEADER_READ_TIMEOUT` | 30 s | A client that does not send a complete request head in time is disconnected. Idle keep-alive connections close after the same time. |
| `CONNECT_TIMEOUT` | 10 s | A TCP connect to the upstream that does not complete in time fails with `502`. DNS and the TLS handshake are not covered. |
| `POOL_IDLE_TIMEOUT` | 30 s | Unused pooled upstream connections close within about twice this time. |
| `BODY_READ_TIMEOUT` | 300 s | With `--minify-json`, an eligible `POST /v1/messages` body must arrive in time after its inspection permit is granted, or the request fails with `408`. The deadline covers only the body read of the buffered part, up to `MAX_INSPECT_BYTES`. The session replay after the read has no deadline, and a state failure there returns `500`. For a larger body the permit is released and the rest streams without a deadline, like a request that is not inspected. The request is never forwarded unrewritten because of the timeout. |

With `--minify-json`, two more constants in `crates/deslag-proxy/src/minify.rs` apply to eligible `POST /v1/messages` requests:

| Limit | Default | Effect |
| --- | --- | --- |
| `MAX_INSPECT_BYTES` | 32 MiB | A larger body is forwarded unrewritten: the buffered prefix, then the rest as it arrives. Its pinned encoded results go out in original form, so that request loses the cached prefix, and its new results stay undecided. |
| `INSPECT_PERMITS` | 4 | At most 4 requests buffer and rewrite at once. Further eligible requests wait for a permit. `BODY_READ_TIMEOUT` bounds how long a permit holder can wait for its body. The permit is held until the replay of the buffered body ends, also when the client disconnects. The replay parses and rebuilds the body without a time limit and runs one job per text of a new result in sequence. The executor budget below bounds each job, not the whole hold. |

Compression in the proxy and in `deslag hook claude --minify-json` runs under `BoundedExecutor::default()`. The proxy takes it from `Limits::executor`. Each text is one job. In the hook a text is the Bash stdout or one MCP text block. In the proxy a text is one text item of a new eligible Bash or MCP `tool_result` content array, or its string content:

| Limit | Default | Effect |
| --- | --- | --- |
| `max_parallel_jobs` | 4 | A job that finds every slot taken is rejected as `busy`. |
| `processing_budget` | 10 ms | A job that runs longer is rejected as `budget_exceeded`. |

A rejected or panicked job fails open. The proxy records that new tool result as `unchanged` in the session manifest and replays that decision on every resend. A result already pinned as json-minify is re-encoded outside the executor, so a rejected job never changes already-sent history. The hook leaves the tool output unchanged and writes `deslag: compression <busy|budget_exceeded|job_panicked>; tool output unchanged` to stderr. A panicked job also prints the default Rust panic message to stderr before that line.

No limit applies to a response body after the upstream has answered, so long streams are never cut. Other request bodies stream and are not time-bounded: a client that never finishes its body keeps its slot. Zero or out-of-range limit values are rejected at startup.

## Compression owner and proxy replay

Exactly one component compresses a session while the owner stays fixed for that session: the hook, the proxy or neither. Change the owner only between sessions. The owner lives in `<state dir>/owner` as `hook`, `proxy` or `none`. The state dir is `DESLAG_STATE_DIR`, else `$XDG_STATE_HOME/deslag`, else `$HOME/.local/state/deslag`. Empty and relative values are skipped, so the working directory does not change the dir. Each process resolves the dir from its own environment, so the hook and the proxy read the same owner file only when both processes share `DESLAG_STATE_DIR`, `XDG_STATE_HOME` and `HOME`. A missing, unreadable or unknown owner means `none`, and the owner is never inferred from tool output. The plugin command `/deslag:compression on` writes `hook`, and `deslag disable` writes `none`. No command writes `proxy` yet; create the file by hand. When neither `DESLAG_STATE_DIR` nor `XDG_STATE_HOME` is set, for example: `mkdir -m 700 -p ~/.local/state/deslag && echo proxy > ~/.local/state/deslag/owner`.

- `deslag hook claude --minify-json` replaces a result only when the owner is `hook`. The gate applies to every routed field, Bash stdout and MCP text blocks alike; with any other owner the hook emits no replacement and runs no conflict scan. The hook keeps no replay state, because Claude Code stores the replacement and resends it on resume and fork. Another `PostToolUse` hook that matches the same tool blocks the replacement. List the keys of plugins whose hooks only observe in `<state dir>/ignore-hooks`, one per line, to skip their hooks in that scan; see [Hook conflict reporting](docs/compatibility.md#hook-conflict-reporting).
- `deslag proxy --minify-json` needs a state dir and fails at startup without one. It rewrites only requests that carry an `X-Claude-Code-Session-Id` header. Other requests pass through unchanged.

In proxy mode, each session gets a manifest in `<state dir>/sessions/<sha256 of the session id>.json`. The proxy writes it on the first request of a session that carries no tool results, and only while the owner is `proxy`. The manifest pins one decision per tool result, `unchanged` or `json-minify` at the version deslag-core pins, with a SHA-256 digest of the whole tool-result content (`cache_control` on its blocks excluded), the policy version and the request model. It holds no tool output. The proxy commits each new decision with a temp file, `fsync` and rename before it forwards the request. Resent history replays the pinned decision, so the bytes the upstream sees stay the same across resend, proxy restart and model switch. A session that was started under another owner has no manifest and passes through while the owner is not `proxy`. After the owner changes to `proxy`, its requests with tool results get the `400` error below. When the owner changes away from `proxy` mid-session, pinned decisions keep replaying and new results are recorded as `unchanged`; they stay unchanged after the owner changes back. After a change from `proxy` to `hook`, the proxy keeps replaying its earlier pinned decisions while the hook compresses new results, so both transform that session. Start a new session after an owner change. The state dir is created with mode `0700` and its files with `0600`. Commits are serialized within one proxy process only: two proxies on one state dir each replace the manifest atomically, and one can overwrite the new decisions of the other.

The proxy rejects a request with `400 invalid_request_error` and does not forward it when the replay state cannot reproduce the history: the request carries tool results and the session has no manifest (for example a session started before the proxy, a session started under another owner before the owner changed to `proxy`, a fork, or a pruned manifest), the manifest is corrupt, has an unsupported version or lacks a valid owner and mode, a pinned decision is invalid or names a transform this binary does not keep, a known result changed, or an older result was never recorded. Start a new session to recover. A failed commit or a manifest read error other than a missing file returns `500 api_error`. At startup the proxy removes manifests not modified for 30 days (`RETENTION` in `crates/deslag-proxy/src/session.rs`); a later request in such a session gets the `400` error. Pruning runs only at proxy start, so the number of manifests and the decisions per manifest are not bounded while one proxy keeps running.

## Config, launcher, status, doctor and disable

The config file is `DESLAG_CONFIG`, else `$XDG_CONFIG_HOME/deslag/config.toml`, else `$HOME/.config/deslag/config.toml`. Empty and relative values are skipped. A missing file means the defaults. The file uses the keys of [PLAN §11](docs/PLAN.md#11-configuration-and-rollout-policy), and `config_version = 1` is required. Every other key is optional. Today `deslag proxy` and `deslag run` read only `proxy.listen` and `proxy.upstream`, and `status` and `doctor` also print the enabled transforms. Only `deslag proxy --minify-json` turns compression on. Validation covers the `[compression]` thresholds and the other keys. An invalid file stops `deslag proxy` and `deslag run`. The error names the key, or the line number for a TOML syntax error. The file must be consistent on its own: the check that `proxy.upstream` does not point at `proxy.listen` uses the file's values, also when `deslag proxy` overrides one of them with a flag. Validation rejects:

- an unsupported or missing `config_version`, and an unknown key or section;
- a value of the wrong type, `compression.min_candidate_bytes` above `compression.max_candidate_bytes`, `compression.min_estimated_savings_ratio` outside 0 to 1, a negative number, and zero for a count, size or time whose name does not start with `min_`;
- a value that this version cannot honour: `mode` other than `observe`, `transforms.json_minify = false`, `telemetry.content_capture = true`, `telemetry.local_reports = false`, `telemetry.otlp_enabled = true`, and an enabled transform other than `json_minify`;
- a `proxy.listen` that is not a loopback `IP:port`, and a `proxy.upstream` that does not parse as a URL, has no host, is not `https` (plain `http` only to a loopback host), has credentials, a query or a fragment, has an invalid or zero port such as `https://gw.example:0`, has a non-canonical numeric host such as `127.1`, or points at the listener (a loopback or unspecified address, `localhost` or a `*.localhost` name on the listener port).

```toml
config_version = 1

[proxy]
listen = "127.0.0.1:8789"
upstream = "https://api.anthropic.com"
```

- `deslag run [--] <command> [args...]` runs the command with `ANTHROPIC_BASE_URL=http://<proxy.listen>` in the child's environment only. It edits no settings file and leaves the calling shell unchanged. It fails before the child starts when the config is invalid or no proxy accepts a TCP connection on `proxy.listen`. It exits with the child's status, or 127 when the command is not found and 126 when it cannot run. When the parent's `ANTHROPIC_BASE_URL` differs from the proxy URL, it prints a notice with the redacted old value on stderr. Start the proxy first, for example `deslag proxy --minify-json`, write `proxy` to the owner file, then run `deslag run -- claude`.
- `deslag status` prints the version, the config path and its validity, the compression mode with its gate, the owner, the enabled transforms, the proxy listener and upstream, the state dir, and whether the `deslag local` service answers. The transforms, proxy listen and proxy upstream lines appear only when the config is valid. The mode is `hook mode (owner hook, /deslag:compression on)`, `proxy mode (owner proxy; compresses only through deslag proxy --minify-json)`, `off for the plugin hook (owner hook; a hook that runs deslag hook claude --minify-json still compresses)` or `off`. The plugin hook passes `--minify-json` only while the toggle is on, but a project hook that runs `deslag hook claude --minify-json` compresses whenever the owner is `hook`. Proxy mode compresses nothing when the proxy runs without `--minify-json` or the session does not go through it.
- `deslag doctor` prints the status facts and checks the state dir (owned by the current user, mode `0700`; a missing dir is fine and means compression is off), the config, the owner against the compression toggle, and `ANTHROPIC_BASE_URL` against `http://<proxy.listen>`. It exits 1 when a check fails. A toggle `on` with owner `proxy` is a warning, and a toggle `on` with owner `none` is a failure. It prints `set (redacted)` for `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, a URL with userinfo and a value that does not parse as an absolute URL, and any other printed URL keeps `scheme://host[:port]` and prints a path or query as `/[redacted]`.
- `deslag disable` writes `none` to `<state dir>/owner` and `off` to `<state dir>/compression`, each atomically. New tool results stay unchanged from then on, with the hook and with the proxy. Results already compressed or pinned in the current session stay as they are, and the proxy keeps replaying its pinned decisions. Start a new Claude Code session for an unchanged history.

## Roadmap

| Milestone | Scope | Epic |
| --- | --- | --- |
| M0 | Compatibility spike: no-op hook, transparent proxy, session ADR, first JSON-minification slice | [#2](https://github.com/mfittko/deslag/issues/2) |
| M1 | Compression core, limits, token estimation, evaluation harness | [#3](https://github.com/mfittko/deslag/issues/3) |
| M2 | v0.1 release: production hook, replay manifests, proxy mutation, CLI, packaging | [#4](https://github.com/mfittko/deslag/issues/4) |
| M3 | v0.2: OpenTelemetry export, OpenAI adapters | not yet filed |

## Development

Requirements: rustup. `rust-toolchain.toml` pins Rust 1.96.0 with rustfmt and clippy. A cargo installed without rustup ignores the pin.

Baseline checks (CI runs the same commands):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

The Claude Code plugin lives in [`plugins/claude`](plugins/claude). It registers a `PostToolUse` hook and an opt-in usage module. Hook compression is off by default, so the hook leaves tool results unchanged until you run `/deslag:compression on`. The usage module is off by default (see below). Install it from this repository's marketplace in Claude Code:

```text
/plugin marketplace add mfittko/deslag
/plugin install deslag@deslag
```

The plugin does not download anything on its own. Run `/deslag:install` once after you install or update the plugin. The command downloads the release archive for the plugin version and your platform (macOS arm64 or Linux x86_64), verifies it against the release `SHA256SUMS` and installs `deslag` into the private plugin data directory. It never replaces a `deslag` binary on your `PATH`. Until the matching binary is installed, the hook exits without output and tool results stay unchanged.

- `/deslag:compression on` turns hook compression on: the hook then runs `deslag hook claude --minify-json`. It writes `on` to `<state dir>/compression` and `hook` to `<state dir>/owner`. After a change from the `proxy` owner, the current session has two adapters: the proxy replays its pinned results and the hook compresses new ones. Start a new Claude Code session. `/deslag:compression off` writes `off` and leaves the owner unchanged; results that the hook already replaced stay replaced. `/deslag:compression status` reports the toggle, the owner and the binary. The command and the hook resolve the state dir from the environment of Claude Code, so start Claude Code with the same `DESLAG_STATE_DIR` as the `deslag local` service.
- If you added a project hook that runs `deslag hook claude --minify-json` by hand, remove it. With the toggle on, both hooks compress.
- `/deslag:status` reports whether the matching binary is installed and how to fix it.
- `/deslag:uninstall` removes the installed binary and then the plugin data directory. It leaves the directory in place when it holds files that `/deslag:install` did not create. `/plugin uninstall deslag@deslag` also removes the data directory.

For a local checkout, `claude --plugin-dir plugins/claude` loads the plugin. Its hook also needs `/deslag:install`, or a binary built with `cargo build --release -p deslag-cli` and copied to `<data dir>/bin/<version>/deslag`. The data dir is `${CLAUDE_CONFIG_DIR:-$HOME/.claude}/plugins/data/<plugin id>/`. The plugin id is `deslag-deslag` for the marketplace install; for another install, list `plugins/data/` under the config dir. Claude Code does not export the plugin data path to your shell, so use the concrete path. After a manual copy, run `touch "<data dir>/.deslag-data"` so that `/deslag:uninstall` recognizes the data directory and removes the binary.

The plugin also holds an opt-in usage module. It sends the provider token usage of each model request to the `deslag local` service, and it never changes the model stream. Run `deslag local start`, then start Claude Code with `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 DESLAG_USAGE_STATS=1`. `deslag stats --json --session <id>` then lists each request with its usage and the estimated tokens that hook compression removed. See [Observability](docs/observability.md#usage-module).

The usage module also holds an opt-in savings widget. Add `DESLAG_SAVINGS_WIDGET=1` to the variables above. After each model request, the status line then shows the tokens that hook compression removed from that request next to its provider-reported input tokens, plus the session total of the last 24 hours and whether hook compression is on. The removed count is an estimate from a local estimator, and the widget labels it as one. The widget shows no money figures. When the service is not running or the module is off, it shows nothing. The [demo guide](docs/demo.md) enables hook compression and the widget in a throwaway session. Its [recorded run](docs/demo.md#recorded-run) shows non-zero savings.

Benchmarks: `benchmarks/run.sh > docs/benchmarks/baseline.md` runs all runners in release mode and records machine and commit metadata. The committed [baseline](docs/benchmarks/baseline.md) is a measurement only; PLAN §13.2 budgets stay provisional.

Work runs through [dev-loops](https://github.com/mfittko/dev-loops). Repository settings live in [`.devloops`](.devloops).

## License

deslag is dual-licensed under the [MIT License](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at your option (`MIT OR Apache-2.0`). The license was decided in the [M0 epic](https://github.com/mfittko/deslag/issues/2).

Unless you explicitly state otherwise, any contribution you intentionally submit for inclusion in deslag is dual-licensed as above, without any additional terms or conditions.

Upstream reuse obligations for Headroom and llm-proxy are recorded in [ADR 0004](docs/decisions/0004-reference-revisions-and-licenses.md).
