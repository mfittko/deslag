### Added

- `deslag_proxy::session` stores the compression owner (`hook`, `proxy` or `none`) in `<state dir>/owner` and keeps per-session replay manifests that pin one decision per tool result, committed atomically and removed after 30 days without use (a replay that commits nothing refreshes the manifest mtime); nothing calls it yet (#18)
