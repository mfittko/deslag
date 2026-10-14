### Added

- `<state dir>/ignore-hooks` lists plugin keys whose `PostToolUse` hooks the hook conflict scan skips (#97)

### Fixed

- `deslag stats` counts a hook result blocked by a hook conflict as `skipped` with reason `hook_conflict` instead of `changed`; restart a running `deslag local` service after the upgrade, because an older service drops reports with the new reason (#97)
