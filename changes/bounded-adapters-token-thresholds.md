### Added

- Every transform applies the context token gain thresholds `min_estimated_token_savings` (default 64) and `min_estimated_savings_permille` (default 100, ratio 0.10) when a token counter is set, and skips with `insufficient_gain` below them (#63)

### Changed

- `deslag hook claude --minify-json` and the proxy run compression under the bounded executor (4 parallel jobs, 10 ms per job); a busy, expired or panicked job leaves the hook output unchanged with the stderr diagnostic `deslag: compression <busy|budget_exceeded|job_panicked>; tool output unchanged` and forwards the original proxy body (#63)
- The proxy moves request rewriting from the async worker threads to the blocking thread pool. The inspection permit stays held until the rewrite ends, also when the client disconnects; `deslag_proxy::Limits` gains an `executor` field and is no longer `Copy` (#63)
