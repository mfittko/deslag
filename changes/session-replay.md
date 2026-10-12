### Changed

- `--minify-json` on `deslag hook claude` and `deslag proxy` now requires an owner file in the state dir: each compresses only when `<state dir>/owner` names that component. An existing `--minify-json` setup without an owner file stops compressing after upgrade (#18)
- `deslag proxy --minify-json` now needs a state directory and fails at startup without one. It rewrites only requests with an `X-Claude-Code-Session-Id` header and removes manifests unused for 30 days at startup (#18)

### Added

- The proxy pins one decision per tool result in a local session manifest, replays it on every later request and returns `400` when the replay state cannot reproduce the history (#18)
