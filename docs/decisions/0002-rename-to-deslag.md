# 0002: Rename the project from deflate to deslag

**Status:** accepted
**Date:** 2026-10-10
**Decided by:** mfittko

## Context

The working name "deflate" reads as HTTP/gzip DEFLATE compression. PLAN §1 has to state that transport compression is not the product. The crates.io name `deflate` belongs to an unrelated compression crate.

PLAN §15 requires namespace checks and forbids a silent rename.

## Decision

The product, repository, binary and plugin are named **deslag**. Slag is the waste skimmed off molten metal. The name describes removing representation overhead from tool results.

Namespace findings on 2026-10-10:

| Namespace | Result |
| --- | --- |
| GitHub `mfittko/deslag` | Free; this repository is renamed to it |
| crates.io `deslag` | Taken: an unrelated LLM-prose linter (github.com/webern/deslag) |
| crates.io `deslag-core`, `deslag-cli` | Free |
| npm `deslag` | Free |
| Domain `deslag.io` | Unregistered at check time |

Naming scheme:

- Crates: `deslag-core`, `deslag-runtime`, `deslag-telemetry`, `deslag-cli`. The `deslag-cli` crate builds a binary named `deslag`.
- Metric names use the `deslag.` prefix.
- The format wrapper key is `_deslag`.

## Consequences

- `cargo install deslag` installs the unrelated linter. Docs must use `cargo install deslag-cli`.
- A user with both crates installed has two `deslag` binaries on PATH. The plugin and GitHub-release installs are unaffected.
- GitHub redirects old `mfittko/deflate` URLs, so existing issue and plan links keep working.
