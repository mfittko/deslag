# 0007: Claude Code mod integration: hybrid now, mod-first compression after live gates

**Status:** accepted — 2026-10-14 ([PR #87](https://github.com/mfittko/deslag/pull/87))
**Date:** 2026-10-14
**Decided by:** mfittko

## Context

Issue #86 asks whether Claude Code Mods should replace the PostToolUse settings hook and the Anthropic HTTP proxy as deslag's primary Claude integration, and where instrumentation belongs. The spike record is [docs/spikes/0086-claude-mod-first.md](../spikes/0086-claude-mod-first.md). Its evidence ledger separates documented, observed, offline-verified and live-verified claims.

Established offline on Claude Code 2.1.296 and 2.1.299 (`claude plugin test`, no model): a `session.append` hook on the `tool-result` door rewrites the stored row; a bridge to the native binary through `$.process.run` fails open on every tested malformed response and row shape; another rewriting mod can change the row after deslag's proposal; a `turn.step` observer forwards chunks and the final result. The native bridge (`deslag codec --protocol 1`) is at or below the hook's p95 cost from 2 KiB to 1 MiB.

Live on 2.1.299 (USD 0.024 of an operator-authorized USD 1.00 cap, haiku only): the model reads the rewritten row, and it survives `--resume`, `--continue` and `--fork-session` with the mod disabled. The client cuts large Bash output before both the hook and the mod, so that limit does not separate them. `$.process.run` cuts stdout silently at 4 MiB and adds about 1 ms per call.

Remaining limits of the mod: at this boundary Bash `stdout` and `stderr` arrive joined, so Bash rows with stderr pass through, except that when `stdout` is empty and `stderr` alone is one JSON document, the mod cannot tell it from `stdout` and minifies it, which the hook never does (a gate item for the opt-in mod compression step in [docs/phases/phase-mod-1.md](../phases/phase-mod-1.md)); the transcript keeps the original structured record; `$.process.run` is CLI only. Live usage data from `turn.step` is not yet captured.

## Decision

1. Claude compression stays with the PostToolUse hook for now. The proxy code and its acceptance criteria are kept.
2. Instrumentation for Claude Code moves into a mod once a live capture confirms that `turn.step` reports usage as declared. It lives in one installable plugin with separate internal modules for compression and observation (Option A). A second installable observability mod is not built unless a consumer outside deslag needs it.
3. Mod compression becomes an opt-in alternative to the hook only after live verification of: the model reading the rewritten row and its stability across resume, continue and fork; the large-output preview ordering; and the `$.process.run` output limit and overhead. These three checks passed on 2.1.299 in the spike, so the opt-in mod compression step may proceed. The default is decided later from a paired evaluation.
4. Ownership extends the existing owner file with the value `mod`. The hook and the proxy already read that value as `none`.
5. The bridge is a subprocess with a versioned stdin/stdout envelope. No daemon, WASM port or JavaScript codec.

## Consequences

- No change to v0.1 release promises, default codecs or the frozen evaluation.
- The staged plan in the phase plan, [docs/phases/phase-mod-1.md](../phases/phase-mod-1.md), defines the follow-up PRs: remaining live verification, `Owner::Mod`, mod instrumentation feeding the #21 local service, then opt-in mod compression.
- A mod does not keep original outputs off disk. Documentation must not present it as redaction.
- `$.process.run` is CLI only. Surfaces without it get no mod compression and fall back to the hook or pass through.
- The bridge must validate a complete envelope, because `$.process.run` truncates stdout without an error.
- The experimental mod in `plugins/claude-mod-experimental/` is not in the marketplace and defaults to off.
