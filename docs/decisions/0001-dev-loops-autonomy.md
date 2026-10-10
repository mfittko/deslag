# 0001: dev-loops runs with maximum autonomy

**Status:** accepted
**Date:** 2026-10-10
**Decided by:** mfittko

## Context

deflate is a new single-maintainer repository. Work runs through dev-loops from the epic tree in issues #1 to #23. The maintainer wants the loop to proceed without operator checkpoints.

## Decision

- `autonomy.stopAt: []` and `autonomy.humanMergeOnly: false`. The agent merges after the full gate pipeline is clean: `draft_gate` and `pre_approval_gate` at the current head, zero unresolved threads, and a green evidence audit.
- Light mode is set to 2 files and 40 lines. This matches the dev-loops repo's own micro-PR ceiling (dev-loops ADR 0108). Rust supply-chain and release surfaces (`Cargo.lock`, `Cargo.toml`, `rust-toolchain.toml`, `deny.toml`, `plugins/claude/**`), the autonomy config (`.devloops`) and the CI workflows (`.github/workflows/**`) always get full gate fan-out.
- A standing ADR-tripwire waiver authorization is granted by mfittko from 2026-10-10 until 2027-01-08 (the 90-day maximum). It applies only to tripwires that record no new decision.

## Consequences

- No human approves merges, so gate quality is the only merge control. All gates stay mandatory.
- Renewing the standing authorization after 2027-01-08 needs a new decision record.
- To return to human merges, set `humanMergeOnly: true` or add `merge` to `stopAt`.
