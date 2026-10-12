### Added

- `deslag hook claude --minify-json` also minifies JSON in the text blocks of MCP tool results; Bash image results, stderr and all other fields and tools stay unchanged (#17)
- `deslag hook claude --minify-json` leaves the output unchanged and reports on stderr when another `PostToolUse` hook in user, project or enabled-plugin settings may also replace it (#17)
