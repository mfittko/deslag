# 0004: Pin reference revisions and record license obligations

**Status:** accepted
**Date:** 2026-10-11
**Decided by:** mfittko

## Context

PLAN §0 requires an inspection of the repository and the upstream reference licenses before any structure change. PLAN §15 requires an inspection of the license and notices at the exact pinned revision before Headroom or `llm-proxy` source is reused. Epic #2 decided the project license `MIT OR Apache-2.0`. DF-001 (#5) records these facts before any code is copied.

## Decision

### Repository inventory

With this change merged, the repository tracks these files:

| Path | Purpose |
| --- | --- |
| `.devloops` | dev-loops repository settings |
| `.github/workflows/ci.yml` | `ci` workflow: `.devloops` validation, `cargo fmt`, `cargo clippy`, `cargo test` |
| `.gitignore` | Ignores `/target/` and `/tmp/` |
| `Cargo.toml` | Virtual workspace, `members = ["crates/*"]`, `license = "MIT OR Apache-2.0"` |
| `Cargo.lock` | Lockfile; the workspace has no dependencies |
| `rust-toolchain.toml` | Toolchain pin `1.96.0` with `rustfmt` and `clippy` |
| `crates/deslag-core/` | The only member crate (library, `publish = false`) |
| `README.md` | Project overview |
| `LICENSE-MIT` | MIT license text |
| `LICENSE-APACHE` | Apache License 2.0 text |
| `docs/PLAN.md` | Implementation plan and backlog |
| `docs/decisions/0001` to `0003` | Earlier decision records |
| `docs/decisions/0004-reference-revisions-and-licenses.md` | This record |

This change adds `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates/deslag-core/`, `LICENSE-MIT`, `LICENSE-APACHE` and this record. It edits `.github/workflows/ci.yml`, `.devloops` and `README.md`.

### Pinned reference revisions

Each revision is the head of the default branch on 2026-10-11, resolved with `gh api repos/<owner>/<repo>/commits/main`.

| Ref | Repository | Branch | Commit | Commit date (UTC) |
| --- | --- | --- | --- | --- |
| S1, S2 | `headroomlabs-ai/headroom` | `main` | `976aa714ef3e19174dc0cb586cee7b20d064e4ef` | 2026-10-10 16:06:21 |
| S3, S4 | `sofatutor/llm-proxy` | `main` | `4ad75ccf8a3b216872a83a518fe3d9e2b68574b9` | 2026-03-28 11:24:19 |

The PLAN §20 paths exist at these commits: `crates/headroom-core/src/transforms/mod.rs`, `docs/observability/instrumentation.md` and `internal/eventbus/eventbus.go`.

### License findings

Headroom at `976aa71`:

- `LICENSE` is the Apache License 2.0. The appendix reads `Copyright 2025 Headroom Contributors`.
- The workspace `Cargo.toml` sets `license = "Apache-2.0"`. `headroom-core` and `headroom-proxy` inherit it with `license.workspace = true`.
- `NOTICE` exists at the root. It reads `Headroom` / `Copyright 2025 Headroom Contributors` / `This product includes software developed by the Headroom Contributors.` It then lists third-party licenses: tiktoken (MIT), Pydantic (MIT), sentence-transformers (Apache-2.0, optional), FastAPI (MIT, optional), NumPy (BSD-3-Clause, optional), and vendored dashboard assets Tailwind CSS 3.4.17 (MIT) and Alpine.js 3.13.3 (MIT) under `headroom/dashboard/static/`.
- `plugins/headroom-oauth2/LICENSE` declares `SPDX-License-Identifier: Apache-2.0` and links to the full text.
- A recursive tree search found no other `LICENSE`, `NOTICE`, `COPYING`, `THIRD_PARTY` or `AUTHORS` files.
- `sbom/headroom-python-licenses.csv` is a third-party license inventory. It covers the Python distribution only.

llm-proxy at `4ad75cc`:

- `LICENSE` is the MIT License with `Copyright (c) 2025 Manuel Fittko, sofatutor GmbH`.
- A recursive tree search found no `NOTICE` file and no other license files.

### Reuse obligations

These obligations apply when a later change copies or adapts upstream source:

- Headroom (Apache-2.0, section 4): ship a copy of the Apache License 2.0 with the work. Add a prominent notice to every modified file that states it was changed. Keep all copyright, patent, trademark and attribution notices from the source. Carry the attribution notices of the Headroom `NOTICE` file forward (section 4(d)). deslag policy satisfies section 4(d) with a root `NOTICE` file. Third-party entries in that `NOTICE` apply only if the copied code includes or depends on those components.
- llm-proxy (MIT): keep the copyright notice `Copyright (c) 2025 Manuel Fittko, sofatutor GmbH` and the MIT permission notice with every copied or substantial portion.
- Both: record each copied or adapted file with its upstream path and pinned commit, as PLAN §15 requires.

llm-proxy (MIT) source may be distributed under either deslag license. Headroom (Apache-2.0) source can be included. Copied or adapted Headroom files stay under Apache-2.0 only. The release license task in PLAN §15 must confirm this before such code ships.

### No copied source

This change copies no source from Headroom or llm-proxy. deslag therefore has no `NOTICE` file yet.

## Consequences

- The first change that copies upstream source must add the attribution and a provenance record. If the source is from Headroom, it must also add a `NOTICE` file.
- A later reuse of a newer upstream revision must inspect `LICENSE` and `NOTICE` again at that commit and update this record.
- Legal review of the release is still pending in the PLAN §15 release license task. This record lists the observed license texts as input to that task.
