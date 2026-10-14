### Added

- `deslag stats [--json] [--since <n>h|<n>d]` prints the aggregates as a table or as versioned JSON (#21)

### Changed

- The local service counts a report with a byte size above 2^32 - 1 as a service drop, since no real tool result is that large (#21)
- The local service counts a `would_change` report with a null byte or token candidate as a service drop, since deslag-core always sets one in observe mode (#21)
- Rotation removes `stats.jsonl` lines with an hour that is not a multiple of 3600, with partly null dimensions, or with outcome `would_change` and a null byte or token candidate (#21)
