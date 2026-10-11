# deslag

A small, native, deterministic context-compression layer for AI agents.

deslag reduces the representation overhead of tool results before a model sees them. It does not change executed tool calls, it does not silently drop evidence, and telemetry stays off the request path.

> **Status:** early development. The M0 Claude Code plugin and a no-op `PostToolUse` hook adapter (`deslag hook claude`) exist. A first JSON-minification slice is available behind an opt-in `--minify-json` flag on `deslag hook claude` and `deslag proxy`; it applies to Bash results only and is untested against a live client. The [implementation plan](docs/PLAN.md) is the product brief and backlog. Progress is tracked in the [v0.1 epic](https://github.com/mfittko/deslag/issues/1).

## Planned v0.1

- A Rust core (`deslag-core`) with deterministic transforms for model-visible tool results:
  - JSON minification that preserves every value and lexeme (default on).
  - `table-v1` repeated-record encoding for homogeneous JSON arrays (opt-in).
  - Exact consecutive-line run encoding for plain text (opt-in).
- A Claude Code plugin that runs the native binary as a `PostToolUse` hook.
- A minimal transparent Anthropic Messages proxy with streaming support.
- Local compression statistics that separate observed proposals from emitted reductions.

Structural encodings stay opt-in until paired task evaluations pass. See [PLAN §14.4](docs/PLAN.md#144-agent-effectiveness).

## Proxy limits

`deslag proxy --upstream <url> [--listen <addr>] [--minify-json]` runs the proxy. It listens on `127.0.0.1:8789` by default and accepts only loopback addresses. It bounds connections and waiting time. The defaults are constants in `crates/deslag-proxy/src/lib.rs`:

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

The Claude Code plugin lives in [`plugins/claude`](plugins/claude). It currently registers a no-op `PostToolUse` hook that runs `deslag hook claude`, so the `deslag` binary must be on `PATH`. Install it from the repository root with `cargo install --path crates/deslag-cli --locked`.

Benchmarks: `benchmarks/run.sh > docs/benchmarks/baseline.md` runs all runners in release mode and records machine and commit metadata. The committed [baseline](docs/benchmarks/baseline.md) is a measurement only; PLAN §13.2 budgets stay provisional.

Work runs through [dev-loops](https://github.com/mfittko/dev-loops). Repository settings live in [`.devloops`](.devloops).

## License

deslag is dual-licensed under the [MIT License](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at your option (`MIT OR Apache-2.0`). The license was decided in the [M0 epic](https://github.com/mfittko/deslag/issues/2).

Unless you explicitly state otherwise, any contribution you intentionally submit for inclusion in deslag is dual-licensed as above, without any additional terms or conditions.

Upstream reuse obligations for Headroom and llm-proxy are recorded in [ADR 0004](docs/decisions/0004-reference-revisions-and-licenses.md).
