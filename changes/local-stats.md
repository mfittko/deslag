### Added

- `deslag stats [--json] [--since <n>h|<n>d]` prints the aggregates as a table or as versioned JSON (#21)

### Changed

- The local service counts a report with a byte size above 2^32 - 1 as a service drop, since no real tool result is that large (#21)
- Rotation removes `stats.jsonl` lines with an hour that is not a multiple of 3600 or with partly null dimensions, and drops `would_change` reports and lines with a null candidate (#21)
