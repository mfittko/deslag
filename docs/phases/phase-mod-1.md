# Claude Code mod rollout: remaining live checks and staged mod adoption

## Status

Draft (graduated from the issue #86 spike). Needs refinement before promotion.

## Objective

Act on the recommendation of the issue #86 spike. Keep the PostToolUse hook as the Claude compression default, finish the remaining live checks, and add the mod as an opt-in alternative for compression and as the home of Claude instrumentation (Option A, one installable mod with internal modules). [ADR 0007](../decisions/0007-claude-mod-first-integration.md) records the decision.

The evidence ledger (E1 to E21), the benchmark, the comparison table and the live runs live in the spike record, [docs/spikes/0086-claude-mod-first.md](../spikes/0086-claude-mod-first.md). This plan cites them by E-number and does not copy them.

## In scope

Staged plan. Each step is one small PR with its own tests; none deletes the proxy or changes a release promise. Step 1 is the spike itself (PR #87).

| Step | Change | Depends on | Acceptance | Rollback |
| --- | --- | --- | --- | --- |
| 2 | Remaining live verification (see Pending checks) under an operator-authorized cap; record the mod row in [`docs/compatibility.md`](../compatibility.md) | 1, budget authorization | Usage capture, classic-hook ordering and in-session lifecycle recorded with versions, commands and costs | Docs only |
| 3 | `Owner::Mod` in `deslag_proxy::session` and the owner writer in the #20 launcher/doctor work; the hook conflict scan reports an active mod | 1 | Owner file round trip; hook and proxy stand down under `mod` (tests) | Owner back to `hook` |
| 4 | Usage observer and compression counters in `plugins/claude` as an opt-in mod module, feeding the #21 local service over a bounded bridge | 2 (usage capture), #21 | Works with compression off; drops are counted; no awaited I/O in the hot path | Disable the mod module; the hook is unaffected |
| 5 | Mod compression as an opt-in alternative to the hook, binary path from the verified install, one owner selected through the owner file | 3 | #22 and #23 gates extended to the mod adapter, including the stderr-only JSON gate below | Owner `hook`, disable the mod module |
| 6 | Decide the Claude default (hook or mod) and whether the proxy requirement leaves the Claude MVP | 5, a paired evaluation run | Owner decision recorded in a new ADR | Keep the hook default |

Step 5 gate items:

- Stderr-only JSON: when Bash `stdout` is empty and `stderr` alone is one JSON document, the mod cannot tell it from `stdout` and minifies it. The hook's stdout-only rule never touches it (spike comparison table, "Bash with stderr" row). Step 5 must close this gap or keep such rows unchanged before the mod rewrites Bash rows.
- Bash rows with stderr pass through (E9), and originals stay on disk (E15). Documentation must not present the mod as redaction.
- `$.process.run` cuts stdout at 4 MiB without an error (E17). The bridge must keep validating a complete envelope.

Issue impact (proposals, not edits to those issues):

- #18 / PR #72: merged. Manifests stay for the proxy; the mod needs no replay state because the client stores the row (E16).
- #19: no change until step 5. Mod compression is not a reason to declare proxy acceptance criteria met.
- #20: split generic config and diagnostics from proxy launch; add the owner writer for `mod`.
- #21: reuse the report schema and bounded delivery for mod counters.
- #22 / #23: add mod adapter compatibility, fault, security, performance and clean-install gates at step 5; codec and model-quality gates stay.
- #4, #77, #84: no impact on `v0.1.0-alpha.3` or the frozen evaluation.
- Future OTLP: the exporter stays outside the mod and outside the core; default exports stay metadata-only.

Session boundaries for owner changes. The owner file is per state directory, so a change applies to every session that shares it. Hook to mod and mod to hook are safe at any time, because the client stores both forms (E16 for the mod, ADR 0006 for the hook). Proxy to mod keeps working while the proxy stays in the path, since it replays pinned decisions under any owner. Removing the proxy from a session with encoded history needs a new session (ADR 0006).

### Pending checks

- `turn.step` usage in a live session (E18). The observer has no export channel yet, so this needs the step-4 bridge or a probe.
- Ordering against the classic PostToolUse hook in one session, and whether `classic.PostToolUse` lets a mod observe hook outcomes.
- Model switch, reload, upgrade and disable inside a running session; whether a timed-out child is killed.
- Linux x86_64, headless SDK and Desktop surfaces.
- Memory, allocations and parallel tool calls in the bridge benchmark, and behavior under telemetry failure.

### Open operator decisions

- Budget authorization for the step-2 live checks.
- The Claude default (hook or mod) at step 6, from a paired evaluation.
- Whether the proxy requirement leaves the Claude MVP (step 6).

## Explicit non-goals

- Re-running the spike's exploration; that question is concluded.
- Work beyond the recommendation above.
