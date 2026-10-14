### Added

- `deslag stats [--json] [--since <n>h|<n>d]` prints the aggregates as a table or as versioned JSON (#21)

### Changed

- The local service counts a report with a byte size above 2^32 - 1 as a service drop, since no real tool result is that large (#21)
- The local service counts a `would_change` report with a null `candidate_bytes` as a service drop, since deslag-core always sets it in observe mode. A null token candidate is kept, and a token sum that includes one is null (#21)
- Rotation merges `stats.jsonl` lines with equal keys into one line. It removes every line that fails validation: another schema version, a value outside the allowlists, a missing or non-integer field, an hour that is not a multiple of 3600, partly null dimensions, or outcome `would_change` with a null `candidate_bytes`. It removes unknown token labels and keeps the rest of the line. Before, rotation kept any JSON line with a numeric `hour` (#21)
