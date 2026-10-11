### Added

- `deslag proxy --upstream <url> [--listen <addr>] [--minify-json]` runs the proxy from the `deslag` CLI and reports the bound address (#33)

### Changed

- `deslag proxy` replaces the unreleased `deslag-proxy` binary (#33)

### Fixed

- `Upstream` debug output redacts the upstream base path like its display form (#33)
