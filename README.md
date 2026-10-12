# deslag

A small, native, deterministic context-compression layer for AI agents.

deslag reduces the representation overhead of tool results before a model sees them. It does not change executed tool calls, it does not silently drop evidence, and telemetry stays off the request path.

> **Status:** early development. The M0 Claude Code plugin and a no-op `PostToolUse` hook adapter (`deslag hook claude`) exist. A first JSON-minification slice is available behind an opt-in `--minify-json` flag on `deslag hook claude` and `deslag proxy`; it applies to Bash results only and is untested against a live client. One configured owner selects whether the hook or the proxy compresses, and the proxy pins its per-result decisions in local session manifests. The [implementation plan](docs/PLAN.md) is the product brief and backlog. Progress is tracked in the [v0.1 epic](https://github.com/mfittko/deslag/issues/1).

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

`deslag proxy --upstream <url> [--listen <addr>] [--minify-json]` runs the proxy. It listens on `127.0.0.1:8789` by default. `--listen` takes a literal `IP:port`, for example `127.0.0.1:8789` or `[::1]:8789`, and the IP must be a loopback address. It bounds connections and waiting time. The defaults are constants in `crates/deslag-proxy/src/lib.rs`:

| Limit | Default | Effect |
| --- | --- | --- |
| `MAX_CONNECTIONS` | 128 | Further client connections wait in the kernel listen backlog until a slot frees. The backlog size is a separate listener setting, capped by the OS `somaxconn`. |
| `HEADER_READ_TIMEOUT` | 30 s | A client that does not send a complete request head in time is disconnected. Idle keep-alive connections close after the same time. |
| `CONNECT_TIMEOUT` | 10 s | A TCP connect to the upstream that does not complete in time fails with `502`. DNS and the TLS handshake are not covered. |
| `POOL_IDLE_TIMEOUT` | 30 s | Unused pooled upstream connections close within about twice this time. |
| `BODY_READ_TIMEOUT` | 300 s | With `--minify-json`, an eligible `POST /v1/messages` body must arrive in time after its inspection permit is granted, or the request fails with `408`. The deadline covers only the buffered part, up to `MAX_INSPECT_BYTES`. For a larger body the permit is released and the rest streams without a deadline, like a request that is not inspected. The request is never forwarded unrewritten because of the timeout. |

With `--minify-json`, two more constants in `crates/deslag-proxy/src/minify.rs` apply to eligible `POST /v1/messages` requests:

| Limit | Default | Effect |
| --- | --- | --- |
| `MAX_INSPECT_BYTES` | 32 MiB | A larger body is forwarded unrewritten: the buffered prefix, then the rest as it arrives. |
| `INSPECT_PERMITS` | 4 | At most 4 requests buffer and rewrite at once. Further eligible requests wait for a permit. `BODY_READ_TIMEOUT` bounds how long a permit holder can keep it. |

No limit applies to a response body after the upstream has answered, so long streams are never cut. Other request bodies stream and are not time-bounded: a client that never finishes its body keeps its slot. Zero or out-of-range limit values are rejected at startup.

## Compression owner and proxy replay

Exactly one component compresses a session: the hook, the proxy or neither. The owner lives in `<state dir>/owner` as `hook`, `proxy` or `none`. The state dir is `DESLAG_STATE_DIR`, else `$XDG_STATE_HOME/deslag`, else `$HOME/.local/state/deslag`. Empty and relative values are skipped, so the hook and the proxy resolve the same dir from any working directory. A missing, unreadable or unknown owner means `none`, and the owner is never inferred from tool output. No deslag command writes the owner yet; create the file by hand, for example `mkdir -p ~/.local/state/deslag && echo proxy > ~/.local/state/deslag/owner`.

- `deslag hook claude --minify-json` replaces a result only when the owner is `hook`. The hook keeps no replay state, because Claude Code stores the replacement and resends it on resume and fork.
- `deslag proxy --minify-json` needs a state dir and fails at startup without one. It rewrites only requests that carry an `X-Claude-Code-Session-Id` header. Other requests pass through unchanged.

In proxy mode, each session gets a manifest in `<state dir>/sessions/<sha256 of the session id>.json`. The proxy writes it on the first request of a session that carries no tool results, and only while the owner is `proxy`. The manifest pins one decision per tool result, `unchanged` or `json-minify` at the version deslag-core pins, with a SHA-256 digest of the result text, the policy version and the request model. It holds no tool output. The proxy commits each new decision with a temp file, `fsync` and rename before it forwards the request. Resent history replays the pinned decision, so the bytes the upstream sees stay the same across resend, proxy restart and model switch. A session that was started under another owner passes through. When the owner changes away from `proxy` mid-session, pinned decisions keep replaying and new results are recorded as `unchanged`; they stay unchanged after the owner changes back. The state dir is created with mode `0700` and its files with `0600`. Commits are serialized within one proxy process only: two proxies on one state dir never leave a torn manifest, but one can overwrite the new decisions of the other.

The proxy rejects a request with `400 invalid_request_error` and does not forward it when the replay state cannot reproduce the history: the request carries tool results and the session has no manifest (for example a session started before the proxy, a fork, or a pruned manifest), the manifest is corrupt or has an unsupported version, a pinned decision is invalid or names a transform this binary does not keep, a known result changed, or an older result was never recorded. Start a new session to recover. A failed commit or a manifest read error other than a missing file returns `500 api_error`. At startup the proxy removes manifests not modified for 30 days (`RETENTION` in `crates/deslag-proxy/src/session.rs`); a later request in such a session gets the `400` error. Pruning runs only at proxy start, so the number of manifests and the decisions per manifest are not bounded while one proxy keeps running.

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

The Claude Code plugin lives in [`plugins/claude`](plugins/claude). It currently registers a no-op `PostToolUse` hook. Install it from this repository's marketplace in Claude Code:

```text
/plugin marketplace add mfittko/deslag
/plugin install deslag@deslag
```

The plugin does not download anything on its own. Run `/deslag:install` once after you install or update the plugin. The command downloads the release archive for the plugin version and your platform (macOS arm64 or Linux x86_64), verifies it against the release `SHA256SUMS` and installs `deslag` into the private plugin data directory. It never replaces a `deslag` binary on your `PATH`. Until the matching binary is installed, the hook exits without output and tool results stay unchanged.

- `/deslag:status` reports whether the matching binary is installed and how to fix it.
- `/deslag:uninstall` removes the installed binary and then the plugin data directory. It leaves the directory in place when it holds files that `/deslag:install` did not create. `/plugin uninstall deslag@deslag` also removes the data directory.

For a local checkout, `claude --plugin-dir plugins/claude` loads the plugin. Its hook also needs `/deslag:install`, or a binary built with `cargo build --release -p deslag-cli` and copied to `<data dir>/bin/<version>/deslag`. The data dir is `${CLAUDE_CONFIG_DIR:-$HOME/.claude}/plugins/data/<plugin id>/`. The plugin id is `deslag-deslag` for the marketplace install; for another install, list `plugins/data/` under the config dir. Claude Code does not export the plugin data path to your shell, so use the concrete path. After a manual copy, run `touch "<data dir>/.deslag-data"` so that `/deslag:uninstall` recognizes the data directory and removes the binary.

Benchmarks: `benchmarks/run.sh > docs/benchmarks/baseline.md` runs all runners in release mode and records machine and commit metadata. The committed [baseline](docs/benchmarks/baseline.md) is a measurement only; PLAN §13.2 budgets stay provisional.

Work runs through [dev-loops](https://github.com/mfittko/dev-loops). Repository settings live in [`.devloops`](.devloops).

## License

deslag is dual-licensed under the [MIT License](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at your option (`MIT OR Apache-2.0`). The license was decided in the [M0 epic](https://github.com/mfittko/deslag/issues/2).

Unless you explicitly state otherwise, any contribution you intentionally submit for inclusion in deslag is dual-licensed as above, without any additional terms or conditions.

Upstream reuse obligations for Headroom and llm-proxy are recorded in [ADR 0004](docs/decisions/0004-reference-revisions-and-licenses.md).
