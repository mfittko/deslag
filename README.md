# deslag

A small, native, deterministic context-compression layer for AI agents.

deslag reduces the representation overhead of tool results before a model sees them. It does not change executed tool calls, it does not silently drop evidence, and telemetry stays off the request path.

> **Status:** planning. Nothing is implemented yet. The [implementation plan](docs/PLAN.md) is the product brief and backlog. Progress is tracked in the [v0.1 epic](https://github.com/mfittko/deslag/issues/1).

## Planned v0.1

- A Rust core (`deslag-core`) with deterministic transforms for model-visible tool results:
  - JSON minification that preserves every value and lexeme (default on).
  - `table-v1` repeated-record encoding for homogeneous JSON arrays (opt-in).
  - Exact consecutive-line run encoding for plain text (opt-in).
- A Claude Code plugin that runs the native binary as a `PostToolUse` hook.
- A minimal transparent Anthropic Messages proxy with streaming support.
- Local compression statistics that separate observed proposals from emitted reductions.

Structural encodings stay opt-in until paired task evaluations pass. See [PLAN §14.4](docs/PLAN.md#144-agent-effectiveness).

## Roadmap

| Milestone | Scope | Epic |
| --- | --- | --- |
| M0 | Compatibility spike: no-op hook, transparent proxy, session ADR, first JSON-minification slice | [#2](https://github.com/mfittko/deslag/issues/2) |
| M1 | Compression core, limits, token estimation, evaluation harness | [#3](https://github.com/mfittko/deslag/issues/3) |
| M2 | v0.1 release: production hook, replay manifests, proxy mutation, CLI, packaging | [#4](https://github.com/mfittko/deslag/issues/4) |
| M3 | v0.2: OpenTelemetry export, OpenAI adapters | not yet filed |

## Development

Requirements: a Rust toolchain. The toolchain version will be pinned in `rust-toolchain.toml` ([DF-001](https://github.com/mfittko/deslag/issues/5)).

Baseline checks, once the workspace exists:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Work runs through [dev-loops](https://github.com/mfittko/dev-loops). Repository settings live in [`.devloops`](.devloops).

## License

deslag is dual-licensed under the [MIT License](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at your option (`MIT OR Apache-2.0`). The license was decided in the [M0 epic](https://github.com/mfittko/deslag/issues/2).

Unless you explicitly state otherwise, any contribution you intentionally submit for inclusion in deslag is dual-licensed as above, without any additional terms or conditions.

Upstream reuse obligations for Headroom and llm-proxy are recorded in [ADR 0004](docs/decisions/0004-reference-revisions-and-licenses.md).
