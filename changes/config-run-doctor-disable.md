### Added

- A versioned TOML config file (`DESLAG_CONFIG`, else `$XDG_CONFIG_HOME/deslag/config.toml`, else `$HOME/.config/deslag/config.toml`); validation rejects unknown keys, unsupported versions, inconsistent thresholds and unsafe listener or upstream values, and names the key (#20)
- `deslag run [--] <command>` launches a child with `ANTHROPIC_BASE_URL` pointing at the configured proxy, in the child's environment only, and fails with guidance when no proxy listens there (#20)
- `deslag status` and `deslag doctor` report the version, config validity, hook mode, proxy mode or off, transforms and the local service; `doctor` checks the state dir, the owner and the routing, redacts credentials and exits 1 on a failed check (#20)
- `deslag disable` writes owner `none` and compression `off`, and tells you to start a new Claude Code session for an unchanged history (#20)

### Changed

- `deslag proxy` takes a missing `--upstream` or `--listen` from the config file; an invalid config file stops it even with both flags (#20)
