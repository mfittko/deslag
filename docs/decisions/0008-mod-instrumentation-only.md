# 0008: The Claude Code mod is for instrumentation only

**Status:** accepted on 2026-10-14 ([issue #88](https://github.com/mfittko/deslag/issues/88)). Supersedes the mod-compression clause of [ADR 0007](0007-claude-mod-first-integration.md) (decisions 3, 4 and 5).
**Date:** 2026-10-14
**Decided by:** mfittko

## Context

[ADR 0007](0007-claude-mod-first-integration.md) allowed mod compression as an opt-in alternative to the PostToolUse hook. The spike record, [docs/spikes/0086-claude-mod-first.md](../spikes/0086-claude-mod-first.md), showed that a `session.append` hook can rewrite the stored tool-result row. That path duplicates the hook and has more gaps:

- Bash `stdout` and `stderr` arrive joined, so Bash rows with stderr pass through (E9).
- When `stdout` is empty and `stderr` alone is one JSON document, the mod minifies it. The hook never does.
- `$.process.run` cuts the bridge output at 4 MiB without an error (E17).

A mod also cannot rewrite the outgoing request. `turn.step` pins the messages and allows only `model` and `effort` to change. The mod therefore adds no compression capability that the hook or the proxy lacks. Its remaining value is instrumentation, through the provider-reported usage on `turn.step`, and UI.

## Decision

1. The PostToolUse hook stays the Claude compression owner. The proxy stays the request-path adapter.
2. The mod is for instrumentation only. It does not compress tool results.
3. The owner file keeps the values `hook`, `proxy` and `none`. No `mod` value is added.
4. The `deslag codec --protocol 1` bridge, its `mod_bridge` benchmark, the `session.append` compression adapter and the mod's `mode` and `binary` options are removed.
5. ADR 0007 decision 2 still applies: Claude instrumentation moves into a mod once a live capture confirms that `turn.step` reports usage as declared. It lives in one installable plugin (Option A).

## Consequences

- No change to the hook, the proxy, v0.1 release promises, default codecs or the frozen evaluation.
- [docs/phases/phase-mod-1.md](../phases/phase-mod-1.md) keeps only the instrumentation steps and the proxy decision.
- The spike record stays as the historical evidence record. Its commands for the codec bridge and the mod compression adapter no longer run on this revision.
- The experimental mod in `plugins/claude-mod-experimental/` keeps only the `turn.step` usage observer. It is not in the marketplace.
