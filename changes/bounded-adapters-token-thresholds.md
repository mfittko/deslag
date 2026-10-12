### Added

- Every transform applies the context token gain thresholds `min_estimated_token_savings` (default 64) and `min_estimated_savings_permille` (default 100, ratio 0.10) when a token counter is set, and skips with `insufficient_gain` below them (#63)

### Changed

- `deslag hook claude --minify-json` and the proxy run compression under the bounded executor (4 parallel jobs, 10 ms per job); a busy, expired or panicked job leaves the hook output unchanged with a stderr diagnostic and forwards the original proxy body (#63)
- The proxy runs request rewriting on the blocking thread pool instead of the async worker threads; `deslag_proxy::Limits` gains an `executor` field and is no longer `Copy` (#63)
