# 0003: Raise the light-mode cap to 5 files and 200 lines

**Status:** accepted
**Date:** 2026-10-11
**Decided by:** mfittko
**Amends:** ADR 0001 (light-mode values only)

## Context

ADR 0001 set light mode to 2 files and 40 lines, copied from the dev-loops repo's own micro-PR ceiling. In this repo that cap sends almost every PR to a full gate fan-out. The docs-only rename PR (4 files, 127 lines) needed a full fan-out with 4 angles and 2 dispatch units.

## Decision

- `localImplementation.lightMode.maxFiles: 5` and `maxLines: 200`.
- Risk paths are unchanged. Changes to `**/Cargo.lock`, `**/Cargo.toml`, `rust-toolchain.toml`, `deny.toml`, `plugins/claude/**`, `.devloops` and `.github/workflows/**` always get full fan-out, whatever their size.

## Consequences

- Most single-issue docs PRs and small code PRs get one inline reviewer instead of a fan-out.
- Up to 200 lines of Rust core code can be reviewed by a single inline reviewer. If a light-mode PR ships a defect that a fan-out angle would have caught, lower the cap again with a new decision record.
- The other ADR 0001 decisions stay in force.
