### Changed

- `--minify-json` on `deslag hook claude` and `deslag proxy` now requires an owner file in the state dir: each compresses only when `<state dir>/owner` names that component. An existing `--minify-json` setup without an owner file stops compressing after upgrade. Change the owner only between sessions: after a mid-session change from `proxy` to `hook`, the proxy keeps replaying earlier decisions while the hook compresses new results (#18)
- `deslag proxy --minify-json` now needs a state directory and fails at startup without one. It rewrites only requests with an `X-Claude-Code-Session-Id` header and removes manifests not modified for 30 days at startup (#18)

### Added

- The proxy pins one decision per tool result in a local session manifest, replays it on every later request and returns `400` when the replay state cannot reproduce the history (#18)
