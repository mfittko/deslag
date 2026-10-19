### Added

- `deslag status`, `deslag doctor` and `/deslag:compression on|status` list each `PostToolUse` hook that blocks compression of Bash or MCP results, with the line to add to `<state dir>/ignore-hooks` for a plugin hook. `deslag doctor` fails while compression is on with owner hook and such a conflict exists (#103)
