# Observability

deslag can count what it changed, skipped and saved. Counting is optional and local. It needs the `deslag local` service, and `deslag stats` reads the result. Unix only (macOS, Linux).

## Commands

| Command | Effect |
| --- | --- |
| `deslag local start [--retention-days <n>]` | Starts the service in the background and waits until it answers. Exit 0 also when it already runs. |
| `deslag local run [--retention-days <n>]` | Runs the service in the foreground and logs write errors to stderr. Exits 2 when another service holds the lock. |
| `deslag local stop` | Writes all open buckets, removes the socket and stops the service. When the write fails, the service keeps running with its buckets and `stop` exits 2. |
| `deslag local status` | Prints `running` or `not running` and the state directory. |
| `deslag stats [--json] [--since <n>h\|<n>d]` | Asks a running service to write its open buckets, then prints the aggregates of the window as a table, or as JSON with `--json`. The default window is `24h`. Exits 2 when `stats.jsonl` exists but cannot be read, or when stdout cannot be written. |

Retention defaults to 90 days. Malformed arguments print the usage and exit 64.

## State directory

The state directory is `$DESLAG_STATE_DIR`, else `$XDG_STATE_HOME/deslag`, else `$HOME/.local/state/deslag`. Empty or relative values are ignored, as the XDG Base Directory spec requires. The service creates the directory with mode 0700 and sets 0700 on an existing directory. A symlink at that path is followed: its target must be a directory owned by the current user, and the service sets that target to 0700. The directory holds:

- `local.lock`: the single-instance lock (mode 0600). The running service holds an exclusive lock on it, so a second `local run` exits with `service already running`.
- `local.sock`: the service socket (mode 0600). Adapters send one datagram of at most 512 bytes per compression result. Sending never blocks. A missing service or a full buffer drops the report.
- `stats.jsonl`: the aggregate store (mode 0600). When a write fails, the service logs the error to stderr and keeps the buckets in memory for the next hour close, `stop` or `stats`.

## Metric dimensions

Every bucket has these dimensions. Each value comes from a fixed allowlist. The service drops a report with any other value and counts it as a service drop.

| Dimension | Values |
| --- | --- |
| `hour` | Start of the hour, Unix epoch seconds (UTC) |
| `adapter` | `hook`, `proxy` |
| `transform` | `json-minify`, `json-table`, `exact-line-runs`, `line-templates` |
| `outcome` | `changed`, `would_change`, `skipped`, `failed` |
| `reason` | `null`, `mode_off`, `transform_disabled`, `below_min_bytes`, `above_max_bytes`, `unsupported_format`, `depth_limit`, `duplicate_key`, `escaped_key`, `insufficient_gain`, `invalid_candidate` |
| `tool` | `bash`, `mcp`, `other` |

`changed` means deslag emitted the smaller output. `would_change` means observe mode found a smaller candidate and emitted the original. No payload content, session IDs, paths, raw tool names or request IDs are stored.

## JSONL line format

The service appends one line per bucket when an hour closes, and when `stop` or `stats` makes it write the open hour. Lines are additive: the same key can appear on several lines, and readers sum them. Keys are in alphabetical order.

```json
{"adapter":"hook","candidate_bytes":600,"count":2,"dropped":1,"emitted_bytes":600,"hour":1799992800,"original_bytes":1000,"outcome":"changed","reason":null,"tokens":{"estimate:deslag-estimate-v1":{"candidate":150,"emitted":150,"original":250}},"tool":"bash","transform":"json-minify","v":1}
```

| Field | Meaning |
| --- | --- |
| `v` | Line schema version, `1`. |
| `count` | Reports in the bucket. |
| `original_bytes`, `emitted_bytes` | Summed input and output sizes. |
| `candidate_bytes` | Summed candidate sizes of the reports that had a candidate. `null` when none had one. |
| `tokens` | Token sums per `<method>:<tokenizer>` label. The only stored label is `estimate:deslag-estimate-v1`. The method `exact` is reserved for a future tokenizer; the service drops a report that carries it. Only reports with token counts contribute. `{}` means unknown. A token `candidate` is `null` when unknown. Adapters without a token counter send a `null` token candidate for `would_change`. A sum with one `null` token candidate is `null`. |
| `dropped` | Reports the adapter failed to deliver before this one. |

A service line has every dimension `null` and `count` 0. The service writes one for every hour it covered. Its `dropped` counts reports the service rejected in that hour. `stats` treats a closed hour without a service line as a gap.

Lines older than the retention window are removed at service start and after each hour close. This rotation also merges lines with equal keys, so lines from frequent `stats` runs collapse into one line per key. Rotation also removes malformed lines, invalid UTF-8 included, and token labels outside the allowlist; `stats` skips them too. A line is malformed when its `hour` is not a multiple of 3600, or when its dimensions are neither all set (`reason` may be `null`) nor all `null`, or when a `would_change` line has a `null` `candidate_bytes`. A line is also malformed when it has another `v`, a value outside the allowlists, or a missing or non-integer field. The service counts a report with a byte size above 2^32 - 1 as a service drop, since no real tool result is that large. It also drops a `would_change` report with a `null` `candidate_bytes`, since deslag-core always sets it in observe mode. A `null` token candidate is kept.

## `deslag stats --json`

```json
{
  "buckets": [ { "...": "one object per merged JSONL key, without v" } ],
  "partial": { "reasons": ["service_not_running", "drops"], "value": true },
  "schema_version": 1,
  "totals": {
    "dropped": 3,
    "emitted_reduction_bytes": 400,
    "reports": 6,
    "would_change_reduction_bytes": 200
  },
  "window": { "since": 1799992800, "until": 1800000600 }
}
```

| Field | Meaning |
| --- | --- |
| `schema_version` | `1`. A breaking change increments it. |
| `window.since` | Start of the first hour in the window: `now - since`, rounded down to the hour. Epoch seconds. |
| `window.until` | The time of the run. Epoch seconds. |
| `partial.value` | `true` when `reasons` is not empty. |
| `partial.reasons` | Any of `service_not_running`, `drops`, `gaps`, `no_data`, in this order. |
| `totals.reports` | Sum of `count` over all buckets. |
| `totals.emitted_reduction_bytes` | Sum of `original_bytes - emitted_bytes` over `changed` buckets of both adapters. A hook reduction counts once per tool result. A proxy reduction counts once per submission, so history resends count again. `buckets` has the values per adapter. |
| `totals.would_change_reduction_bytes` | Sum of `original_bytes - candidate_bytes` over `would_change` buckets. Every `would_change` bucket has a candidate size, since lines without one are malformed. |
| `totals.dropped` | Sum of `dropped` over all lines, service lines included. |
| `buckets` | Report buckets in the window, merged per key and sorted by hour, then dimensions. Fields as in the JSONL line, without `v`. |

Unknown values are `null`, never 0. Observe-mode proposals (`would_change`) are never added to emitted reductions. Totals saturate at 2^64 - 1.

Partial reasons:

- `service_not_running`: the service did not answer, or it failed to write its open hour. The current hour is missing. A running service writes its open hour before `stats` reads the file.
- `drops`: a line in the window has `dropped` above 0.
- `gaps`: a closed hour in the window has no service line, so the service never ran in that hour. A killed service that restarts within the same hour loses its unwritten reports of that hour without a gap.
- `no_data`: the window has no lines.

The table shows one row per dimension set, summed across the window. `saved` is `original - emitted`, or `original - candidate` for `would_change` rows. `tokens_saved` uses the first token label. `-` means unknown.

## Limitations

- Hook drops are unobservable. A hook process exits after one result, so a failed send is lost without a count.
- The proxy reports once per submission. A conversation resends its history, so the same tool result counts again on every request.
- A killed service loses the open hour since the last write. `deslag local stop` and `deslag stats` write the open hour; there is no signal handling.
- macOS limits socket paths to 104 bytes. A long state directory path makes `local start` fail to bind.
- `deslag local status` and `deslag local stop` exit 0 whether or not the service runs. Their output states whether the service runs. `stop` exits 2 only when the service fails to write its buckets.
