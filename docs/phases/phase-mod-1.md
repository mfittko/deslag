# Claude Code mod rollout: remaining live checks and mod instrumentation

## Status

Draft (graduated from the issue #86 spike). Needs refinement before promotion.

## Objective

Act on the recommendation of the issue #86 spike, as narrowed by issue #88. The PostToolUse hook stays the Claude compression owner, and the proxy stays the request-path adapter. Finish the remaining live checks and make the mod the home of Claude instrumentation (Option A, one installable mod with internal modules). The mod does not compress. [ADR 0007](../decisions/0007-claude-mod-first-integration.md) records the original decision, and [ADR 0008](../decisions/0008-mod-instrumentation-only.md) supersedes its mod-compression clause.

The evidence ledger (E1 to E21), the benchmark, the comparison table and the live runs live in the spike record, [docs/spikes/0086-claude-mod-first.md](../spikes/0086-claude-mod-first.md). This plan cites them by E-number and does not copy them.

## In scope

Staged plan. Each step is one small PR with its own tests; none deletes the proxy or changes a release promise. Step 1 is the spike itself (PR #87). The step numbers keep the spike's numbering. Steps 3 (`Owner::Mod`) and 5 (mod compression) were dropped by ADR 0008.

| Step | Change | Depends on | Acceptance | Rollback |
| --- | --- | --- | --- | --- |
| 2 | Remaining live verification (see Pending checks) under an operator-authorized cap; record the mod row in [`docs/compatibility.md`](../compatibility.md) | 1, budget authorization | Usage capture, classic-hook ordering and in-session lifecycle recorded with versions, commands and costs | Docs only |
| 4 | Usage observer and compression counters in `plugins/claude` as an opt-in mod module, feeding the #21 local service over a bounded bridge | 2 (usage capture), #21 | Works with compression off; drops are counted; no awaited I/O in the hot path | Disable the mod module; the hook is unaffected |
| 6 | Decide whether the proxy requirement leaves the Claude MVP | 1 | Operator decision recorded in a new ADR | Keep the proxy requirement |

Issue impact (proposals, not edits to those issues):

- #18 / PR #72: merged. Manifests stay for the proxy.
- #19: no change. The proxy question is step 6.
- #20: split generic config and diagnostics from proxy launch.
- #21: reuse the report schema and bounded delivery for mod counters.
- #22 / #23: no mod adapter gates; codec and model-quality gates stay.
- #4, #77, #84: no impact on `v0.1.0-alpha.3` or the frozen evaluation.
- Future OTLP: the exporter stays outside the mod and outside the core; default exports stay metadata-only.
- Rust boundaries: `deslag-core` is unchanged, and no CLI bridge is added. Host integration, exporters and protocol state stay outside the core.
- Future harnesses: compression and report contracts stay portable; hook and proxy are delivery alternatives over the same codecs, and the mod carries instrumentation.

### Pending checks

- `turn.step` usage in a live session (E18). The observer has no export channel yet, so this needs the step-4 bridge or a probe.
- Ordering against the classic PostToolUse hook in one session, and whether `classic.PostToolUse` lets a mod observe hook outcomes.
- Model switch, reload, upgrade and disable inside a running session.
- Linux x86_64, headless SDK and Desktop surfaces.
- Observer behavior under telemetry failure.

### Open operator decisions

- Budget authorization for the step-2 live checks.
- Whether the proxy requirement leaves the Claude MVP (step 6).

## Explicit non-goals

- Re-running the spike's exploration; that question is concluded.
- Mod compression and a `mod` owner value (ADR 0008).
- Work beyond the recommendation above.
