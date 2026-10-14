### Added

- `deslag local run [--retention-days N]` runs an optional foreground service that aggregates compression reports into hourly buckets in a private `stats.jsonl` (#21)
- The service answers `ping`, `flush` and `stop` with `ok`. When the `stats.jsonl` write fails, `flush` and `stop` log the error to stderr, answer `flush failed: <error>` and keep the service running (#21)
- The drop count of an hour is the sum of `dropped` over all its `stats.jsonl` lines: report lines carry sender delivery failures and service lines carry service drops (#21)
- `deslag_telemetry::ensure_state_dir` follows a symlinked state directory and refuses a non-directory or a directory not owned by the current user, so `deslag local run` fails in those cases (#21)
