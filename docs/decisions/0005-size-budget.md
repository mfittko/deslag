# 0005: Size budget counts logic lines only, soft limit 800

**Status:** accepted
**Date:** 2026-10-11
**Decided by:** mfittko

## Context

The dev-loops size budget computes `logicLoc = code + testDiscount * test` with a default `testDiscount` of 0.25 and escalates above 400. An escalated merge needs a fresh per-PR owner approval, even under the standing merge authorization.

DF-003 (PR #29) escalated at 409. About 215 of those lines were proxy logic. The rest were blank lines, comment lines the detector missed, the inline `#[cfg(test)]` unit-test module in `src/lib.rs` (counted as code), and 25% of the integration tests in `tests/`.

## Decision

- `gates.size.testDiscount: 0`. Test files do not count toward the budget.
- `gates.size.tiers.default.softLoc: 800`. The waiver threshold stays at 1500 and the absolute ceiling at 2000.

## Consequences

- PRs up to 800 logic lines merge under the standing authorization when the gates are clean. Larger PRs still need a per-PR approval.
- Inline Rust unit-test modules under `src/` still count as code, because the dev-loops classifier works per file. That is a dev-loops limitation, tracked upstream.
- Test-heavy PRs no longer escalate on test volume. Test quality stays with the gate review angles.
