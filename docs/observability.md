# Observability

deslag can count what it changed, skipped and saved. Counting is optional and local. It needs the `deslag local` service, and `deslag stats` reads the result. Unix only (macOS, Linux).

## Commands

| Command | Effect |
| --- | --- |
| `deslag local start [--retention-days <n>]` | Starts the service in the background and waits until it answers. Exit 0 also when it already runs. |
| `deslag local run [--retention-days <n>]` | Runs the service in the foreground and logs write errors to stderr. Exits 2 when another service holds the lock. |
| `deslag local stop` | Writes all open buckets, removes the sockets and stops the service. When the write fails, the service keeps running with its buckets and `stop` exits 2. |
| `deslag local status` | Prints `running` or `not running` and the state directory. |
| `deslag stats [--json [--session <id>]] [--since <n>h\|<n>d]` | Asks a running service to write its open buckets, then prints the aggregates of the window as a table, or as JSON with `--json`. `--session <id>` prints the per-request view of one session; it needs `--json`. The default window is `24h`. Exits 2 when `stats.jsonl` or `requests.jsonl` exists but cannot be read, or when stdout cannot be written. |

Retention defaults to 90 days. Malformed arguments print the usage and exit 64.

## State directory

The state directory is `$DESLAG_STATE_DIR`, else `$XDG_STATE_HOME/deslag`, else `$HOME/.local/state/deslag`. Empty or relative values are ignored, as the XDG Base Directory spec requires. The service creates a missing directory with mode 0700. A symlink at that path is followed. The service refuses the path when it, or the symlink target, is not a directory or is not owned by the current user. Otherwise the service sets the directory, or the symlink target, to mode 0700. The directory holds:

- `local.lock`: the single-instance lock (mode 0600). The running service holds an exclusive lock on it, so a second `local run` exits with `service already running`.
- `local.sock`: the service socket (mode 0600). Adapters send one datagram of at most 512 bytes per compression result. Sending never blocks. A missing service or a full buffer drops the report.
- `local-http.sock`: the HTTP bridge socket (mode 0600) for the usage module. See [Usage module](#usage-module).
- `stats.jsonl`: the aggregate store (mode 0600). When a write fails, the service logs the error to stderr and keeps the buckets in memory for the next hour close, `stop` or `stats`.
- `requests.jsonl`: the per-request usage store (mode 0600). It is written and rotated like `stats.jsonl`. See [Per-request lines](#per-request-lines).

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

`changed` means deslag emitted the smaller output. `would_change` means observe mode found a smaller candidate and emitted the original. Buckets store no payload content, session IDs, paths, raw tool names or request IDs. `requests.jsonl` stores session IDs, turn IDs and model names; see [Per-request lines](#per-request-lines).

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
    "usage": {
      "cache_creation_input_tokens": 5,
      "cache_read_input_tokens": 210,
      "estimated_removed_tokens": null,
      "input_tokens": 30,
      "output_tokens": 5,
      "requests": 3,
      "unknown_usage": 1
    },
    "would_change_reduction_bytes": 200
  },
  "window": { "since": 1799992800, "until": 1800000600 }
}
```

| Field | Meaning |
| --- | --- |
| `schema_version` | `1`. A breaking change increments it. A new field is not a breaking change and keeps the version. |
| `window.since` | Start of the first hour in the window: `now - since`, rounded down to the hour. Epoch seconds. |
| `window.until` | The time of the run. Epoch seconds. |
| `partial.value` | `true` when `reasons` is not empty. |
| `partial.reasons` | Any of `service_not_running`, `drops`, `gaps`, `no_data`, in this order. |
| `totals.reports` | Sum of `count` over all buckets. |
| `totals.emitted_reduction_bytes` | Sum of `original_bytes - emitted_bytes` over `changed` buckets of both adapters. A hook reduction counts once per tool result. A proxy reduction counts once per submission, so history resends count again. `buckets` has the values per adapter. |
| `totals.would_change_reduction_bytes` | Sum of `original_bytes - candidate_bytes` over `would_change` buckets. Every `would_change` bucket has a candidate size, since lines without one are malformed. |
| `totals.dropped` | Sum of `dropped` over all lines, service lines and `requests.jsonl` lines included. |
| `totals.usage` | Usage totals over the `requests.jsonl` lines in the window. See [Usage totals](#usage-totals). |
| `buckets` | Report buckets in the window, merged per key and sorted by hour, then dimensions. Fields as in the JSONL line, without `v`. |

Unknown values are `null`, never 0. Observe-mode proposals (`would_change`) are never added to emitted reductions. Totals saturate at 2^64 - 1.

Partial reasons:

- `service_not_running`: the service did not answer, or it failed to write its open hour. The current hour is missing. A running service writes its open hour before `stats` reads the file.
- `drops`: a line in the window, in either store, has `dropped` above 0.
- `gaps`: a closed hour in the window has no service line, so the service never ran in that hour. A killed service that restarts within the same hour loses its unwritten reports of that hour without a gap.
- `no_data`: the window has no lines.

The table shows one row per dimension set, summed across the window. `saved` is `original - emitted`, or `original - candidate` for `would_change` rows. `tokens_saved` uses the first token label. `-` means unknown. A `usage:` line under the totals shows the usage totals.

## Usage module

The Claude Code plugin ships an opt-in mod module, `plugins/claude/hooks/usage.js`. It observes `turn.step`, one model request, and sends the provider usage of each request to the local service. It never compresses and never changes the stream: every chunk and the final result pass through unchanged. It works with hook compression on or off.

To enable it, start Claude Code with both variables set, and run the service:

```sh
deslag local start
CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 DESLAG_USAGE_STATS=1 claude
```

`CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1` makes Claude Code load mod modules. `DESLAG_USAGE_STATS=1` turns the sends on. Without it, the module sends and counts nothing.

After a request settles, the module starts one background send. The stream never waits for it. The send resolves the state directory as above and posts one JSON record to `http://deslag/usage` over `local-http.sock`:

```json
{"kind":"usage","session":"<session id>","turn":"<turn id>","index":0,"model":"<model>","usage":{"input_tokens":3,"output_tokens":1,"cache_read_input_tokens":7,"cache_creation_input_tokens":0},"dropped":0}
```

`usage` is `null` when the provider reported no usage or only part of it. `dropped` counts earlier records the module failed to deliver.

Drop behavior:

- At most 4 sends are in flight. A request over that cap is dropped and counted.
- A missing service, a refused connection or a non-2xx answer counts as a drop.
- The next delivered record carries the count in `dropped`.

The service accepts one HTTP connection at a time. It reads at most 2048 bytes and must receive the whole request within 500 ms of the accept. It requires `POST /usage` with a `Content-Length` of at most 512 bytes and a body that starts with `{`, and forwards the body as one datagram to `local.sock`. It answers `204` when the datagram was sent, `503` when it was not, and `400` for any other request, and always closes the connection. An invalid record counts as a service drop, like an invalid report.

## Per-request lines

Each valid usage record becomes one line of `requests.jsonl`. The service writes the lines with the buckets: at each hour close, `stop` and `stats`. Keys are in alphabetical order.

```json
{"compression_reports":2,"dropped":1,"estimated_removed_tokens":40,"index":1,"model":"claude-haiku","session":"sess-a","time":1799997000,"turn":"t1","usage":{"cache_creation_input_tokens":0,"cache_read_input_tokens":110,"input_tokens":20,"output_tokens":3},"v":1}
```

| Field | Meaning |
| --- | --- |
| `v` | Line schema version, `1`. |
| `time` | Arrival time at the service, Unix epoch seconds. |
| `session`, `turn` | Session ID and turn ID, 1 to 64 bytes of `[A-Za-z0-9_-]`. |
| `index` | Position of the request inside its turn. |
| `model` | Model name, 1 to 64 bytes of printable ASCII without `"` and `\`. |
| `usage` | Provider-reported token counts, or `null` when unknown. |
| `compression_reports` | Hook reports of this session since its previous request. |
| `estimated_removed_tokens` | Sum of `original - emitted` of the `estimate:deslag-estimate-v1` tokens of those reports. `null` when one report had no token counts. |
| `dropped` | The record's `dropped` field. |

Attribution rule: a hook report carries the session ID of its `PostToolUse` event. The service sums the reports of each session until the next usage record of that session arrives. That record takes the sums, and the sums restart at 0. So a tool result counts toward the model request that first reads it.

Limits of the attribution:

- Subagents of one session share the session ID, so they share the pending sums.
- Proxy reports carry no session and are not attributed. The proxy reports every resent history block on each submission, so per-request attribution would count a result again on every request.
- At most 1024 sessions hold pending sums. A report for a new session over that cap is bucketed but not attributed.
- A report with an invalid session ID (not 1 to 64 bytes of `[A-Za-z0-9_-]`) is bucketed but not attributed. The hook sends no such ID.
- Records and reports travel on separate paths. When the usage record of step N is delayed, it can reach the service after hook reports for step N+1. Then those reports count toward step N, and the attribution is off by one.
- At each hour close, the service forgets the sums of sessions without a report for one hour.

Rotation runs at service start and after each hour close. Lines are in time order, so rotation rewrites `requests.jsonl` only when its first line is malformed or older than the retention window. A rewrite removes the lines older than the window and all malformed lines. Readers skip malformed lines.

## Usage totals

`totals.usage` in `deslag stats --json`:

| Field | Meaning |
| --- | --- |
| `requests` | Request lines in the window. |
| `unknown_usage` | Request lines with `usage` `null`. They add nothing to the token sums. |
| `input_tokens`, `output_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens` | Sums over the request lines with known usage. |
| `estimated_removed_tokens` | Sum over the request lines. `null` when one line has it `null`. |

Estimated removed tokens are estimates from `deslag-estimate-v1`, never provider counts.

## `deslag stats --json --session <id>`

Prints the requests of one session in the window, in file order. An invalid ID, or `--session` without `--json`, prints the usage and exits 64. An unknown ID prints an empty `requests` list.

```json
{
  "partial": { "reasons": ["service_not_running"], "value": true },
  "requests": [
    {
      "compression_reports": 2,
      "dropped": 1,
      "estimated_removed_tokens": 40,
      "index": 1,
      "model": "claude-haiku",
      "time": 1799997000,
      "turn": "t1",
      "usage": { "cache_creation_input_tokens": 0, "cache_read_input_tokens": 110, "input_tokens": 20, "output_tokens": 3 }
    }
  ],
  "schema_version": 1,
  "session": "sess-a",
  "totals": { "...": "totals.usage of this session" },
  "window": { "since": 1799992800, "until": 1800000600 }
}
```

This view is its own document shape with its own `schema_version`, currently `1`; it changes independently of `deslag stats --json`. `window` and `partial` have the same meaning as in `deslag stats --json`. Each request has the fields of its line, without `v` and `session`. `totals` holds the usage totals of this session only.

## Limitations

- Hook drops are unobservable. A hook process exits after one result, so a failed send is lost without a count.
- The proxy reports once per submission. A conversation resends its history, so the same tool result counts again on every request.
- A killed service loses the open hour since the last write. `deslag local stop` and `deslag stats` write the open hour; there is no signal handling.
- macOS limits socket paths to 104 bytes. A long state directory path makes `local start` fail to bind.
- `deslag local status` and `deslag local stop` exit 0 whether or not the service runs. Their output states whether the service runs. Both exit 2 when the service does not answer within the control timeout (`service not responding`) or the control request fails (`control failed: ...`). `stop` also exits 2 when the service fails to write its buckets.
