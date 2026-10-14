### Added

- The Claude Code plugin has an opt-in usage module that sends the provider usage of each model request to the local service. Enable it with `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1` and `DESLAG_USAGE_STATS=1`. Failed and dropped sends are counted (#90)
- The local service accepts usage records over the HTTP socket `local-http.sock` and stores one line per model request in `requests.jsonl` (#90)
- Hook compression reports carry the session ID, and the service attributes them to the next model request of that session (#90)
- `deslag stats --json` reports `totals.usage`, and `deslag stats --json --session <id>` lists the requests of one session with provider usage and estimated removed tokens (#90)

### Changed

- `totals.dropped` in `deslag stats --json` also counts the drops carried by usage records (#90)
