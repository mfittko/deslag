### Added

- `deslag_proxy::session` stores the compression owner (`hook`, `proxy` or `none`) in `<state dir>/owner` and keeps per-session replay manifests that pin one decision per tool result, committed atomically and removed once their mtime is older than 30 days (a replay that commits nothing refreshes the mtime at most once a day, so an idle manifest can be removed up to one day before 30 days without use); nothing calls it yet (#18)
