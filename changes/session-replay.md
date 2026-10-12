### Added

- `--minify-json` on `deslag hook claude` and `deslag proxy` compresses only when the configured owner in `<state dir>/owner` names that component (#18)
- The proxy pins one decision per tool result in a local session manifest, replays it on every later request and returns `400` when the replay state cannot reproduce the history (#18)
- `deslag proxy --minify-json` needs a state directory, rewrites only requests with an `X-Claude-Code-Session-Id` header and removes manifests unused for 30 days at startup (#18)
