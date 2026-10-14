### Changed

- `deslag proxy --minify-json` also minifies MCP tool results, skips results marked `is_error`, and reports the result tool category; the rule applies to new results only, and results already pinned in a session manifest, including those recorded under policy version 1, replay as recorded (#19)
