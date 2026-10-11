# Changelog

All notable changes to deslag are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/). Pending entries live in `changes/`
until a release folds them in; see `changes/README.md` and `docs/release.md`.

## Unreleased

### Added

- Rust workspace with a pinned toolchain and CI that runs fmt, clippy and tests (#27)
- `deslag` Claude Code PostToolUse hook adapter; it is a no-op unless a transform flag is set (#28)
- Transparent streaming reverse proxy for the Anthropic Messages API (#29)
- Proxy connection cap and header-read, connect and pool idle timeouts (#36)
- Decision record for session identity, ownership and replay (#34)
- Opt-in experimental JSON minification of Bash tool output: `deslag hook claude --minify-json` and `deslag-proxy --minify-json`, plus benchmark runners and a measured baseline (#37)
- Release pipeline: changelog fragments, version lockstep check, bump script and a tag-driven release workflow that publishes macOS arm64 and Linux x86_64 archives with SHA256SUMS and build provenance (#39)

### Fixed

- The proxy keeps request paths inside the configured base path (#35)
