# Compatibility

This file records only observed behavior. Untested versions and platforms are unsupported until a run is recorded here.

## Claude Code hook adapter

| Field | Value |
| --- | --- |
| Minimum tested client | Claude Code 2.1.296 |
| Platform | macOS arm64 (Darwin 25.6.0) |
| Hook event | `PostToolUse` |
| Replacement field | `hookSpecificOutput.updatedToolOutput` |
| Tested on | 2026-10-11 |

The shipped `plugins/claude/hooks/hooks.json` holds `"modules"` next to `"hooks"` (#90). Its offline validation and test results are in [Claude Code usage module](#claude-code-usage-module). The earlier 2.1.296 live runs in this file used the plugin before it held `"modules"`. No live hook run covers the mixed file on 2.1.296 to 2.1.298.

### Observed contract

- The client sends one JSON object on stdin. Captured Bash, Read, MCP and subagent payloads are in `tests/fixtures/claude/` (paths and IDs replaced with placeholders).
- A hook that exits 0 with empty stdout leaves the tool output unchanged.
- A Bash replacement whose shape matches the tool's output schema replaces the output the model sees.
- A Bash replacement with the wrong shape is ignored. The client logs `PostToolUse hook returned updatedToolOutput that does not match Bash's output shape: reason=schema_invalid` and sends the original output to the model.
- The client binary documents that hooks run in parallel on the original output and that rewrites from several hooks are last-write-wins. This was read from the client binary and is untested.
- The client persists the replacement, and the original output is absent from the session transcript. `--resume` and `--resume --fork-session` resubmit the replacement. A fork gets a new `session_id` and keeps the parent's `tool_use_id`. Runs and costs are in [ADR 0006](decisions/0006-session-identity-and-replay.md).

### Live runs

Each run used a throwaway project directory, `--setting-sources project,local`, `--strict-mcp-config`, `--plugin-dir` (a throwaway plugin for runs 1-3, a copy of `plugins/claude` for run 4), `--no-session-persistence`, `--max-budget-usd 0.40` and `--output-format json`. No global settings were edited.

| Run | Hook | Client | Model | Result | total_cost_usd |
| --- | --- | --- | --- | --- | --- |
| 1 | Capture script (stdin to file), Bash and Read | 2.1.296 | claude-haiku-5-5 | Payloads captured; fixtures derived | 0.00182775 |
| 2 | Valid Bash replacement (`stdout` set to `REPLACED-BY-HOOK`, other keys kept) | 2.1.296 | claude-haiku-5-5 | Model saw `REPLACED-BY-HOOK`; client logged "replaced tool output" | 0.00121455 |
| 3 | Invalid Bash replacement (bare string) | 2.1.296 | claude-haiku-5-5 | Model saw original `hello-deslag`; client logged the shape mismatch | 0.00042 |
| 4 | `deslag hook claude` through `plugins/claude` | 2.1.296 | claude-haiku-5-5 | Hook ran (`finished with status 0`); output unchanged | 0.0003915 |

Total: USD 0.0038538. The shared M0 live-run budget is USD 5; the operator allotted USD 2.50 of it to this item.

Runs 2 and 3 used throwaway `jq` hooks to characterize the client. The deslag adapter emits replacements only with `--minify-json`. Its `replacement` helper applies the same shape rule, and unit tests cover the accepted and rejected cases.

### JSON minification (opt-in)

`deslag hook claude --minify-json` and `deslag proxy --minify-json` remove insignificant whitespace from JSON in tool results. The hook covers the fields in [Hook field routing](#hook-field-routing). The proxy covers a `tool_result` whose `tool_use_id` names a `Bash` or `mcp__*` tool use and that is not `"is_error": true`. It changes only the string `content` and the `text` of `"type": "text"` content items. Every other block and field stays byte-identical, including `thinking` with its `signature`, `redacted_thinking`, `cache_control`, `is_error` and unknown types and fields. The eligibility rule decides new results; results already pinned in a session manifest, including those recorded under an earlier policy version, replay as recorded. Proxy reports carry the category of the result, `bash` or `mcp`. Both call `deslag_core::compress` with `FIRST_SLICE_JSON_MINIFY_CONTEXT` and commit the output only for a `Changed` result, through `into_changed`. The plugin hook in `plugins/claude` passes the flag only after `/deslag:compression on`; by default it stays a no-op. The hook flag has run against a live client (runs L2 and L3 below). The proxy flag has not. Token savings from whitespace removal have not been measured.

Status: experimental. Each flag compresses only when the configured owner in `<state dir>/owner` names its component, so the hook and the proxy never both transform one session while the owner stays fixed for it. After a mid-session change from `proxy` to `hook`, the proxy keeps replaying earlier pinned decisions while the hook compresses new results. Start a new session after an owner change. The proxy pins per-result decisions in a session manifest and replays them; see [README: Compression owner and proxy replay](../README.md#compression-owner-and-proxy-replay).

- Removing the proxy or its `--minify-json` flag mid-session makes the client resend original results. Content stays correct, but the cached prefix is lost. Start a new session.
- The proxy passes these requests through unchanged and does not replay them: a request with `content-encoding`, a request with a body-integrity header (`content-digest`, `digest`, `content-md5`, `x-amz-content-sha256`, `repr-digest`), a request with trailers, and a body over 32 MiB (`MAX_INSPECT_BYTES`). Trailers are not forwarded upstream. Such a request forwards every tool result in its original form, including results pinned as encoded, so the request itself loses the cached prefix. It is not recorded, so its new tool results stay undecided. A later request in that session that resends them fails with `400`. Start a new session.
- The proxy does not rewrite `/v1/messages/count_tokens`. Its token counts can overstate the rewritten request.
- The proxy buffers and rewrites at most 4 eligible requests at once (`INSPECT_PERMITS`). Others wait for a permit. A body over 32 MiB (`MAX_INSPECT_BYTES`) is forwarded unrewritten. A buffered body that does not arrive within `BODY_READ_TIMEOUT` (300 s) of its permit fails with `408`. The deadline covers only the read of the first 32 MiB. The session replay after the read has no deadline. For a larger body the permit is released and the rest streams without a deadline. The permit stays held through the replay after the read, also when the client disconnects. See the README proxy limits.
- Hook and proxy compress each text as one job of the bounded executor: the Bash stdout or one MCP text block in the hook, one text item or the string content of a new eligible Bash or MCP `tool_result` in the proxy (4 parallel jobs, 10 ms per job). A job that finds every slot taken (`busy`), runs past its budget (`budget_exceeded`) or panics (`job_panicked`) fails open. The hook leaves the tool output unchanged. The proxy records that new tool result as `unchanged` and replays that decision on every resend. The proxy re-encodes a result pinned as json-minify outside the executor, so a rejected job never changes the bytes of already-sent history.

### Hook field routing

With `--minify-json`, `deslag hook claude` passes only these fields through the core. Each field is compressed as its own job of the bounded executor. When any job of one tool output is rejected or panics, every routed field stays unchanged and the hook writes `deslag: compression <busy|budget_exceeded|job_panicked>; tool output unchanged` to stderr. Other fields keep their values. The hook re-serializes the whole `tool_response`. Key order can change. Numbers keep their exact value and digits, including integers outside the 64-bit range. Only an exponent can change form, for example `1e3` becomes `1e+3`.

| Tool | Routed field | Unchanged |
| --- | --- | --- |
| `Bash` | `tool_response.stdout`, skipped when `isImage` is `true` | `stderr`, `interrupted`, `isImage`, `noOutputExpected` and unknown keys |
| `mcp__*` | `text` of each content block with `"type": "text"` | Image, resource and other blocks; other keys in a text block; any `tool_response` that is not an array |
| Every other tool (`Read`, `Edit`, `Agent`, `ToolSearch`, ...) | None | The whole result |

The hook emits one complete replacement or nothing. A replacement whose shape differs from the original `tool_response` is dropped.

Observed MCP shape (live capture, Claude Code 2.1.296, fixtures `posttooluse-mcp-text.json` and `posttooluse-mcp-mixed.json`):

- `tool_response` is an array of Anthropic content blocks, for example `[{"type":"text","text":"..."}]`. Images arrive as `{"type":"image","source":{"type":"base64",...}}`.
- The event carries `mcp_server` with `name` and `source`.
- When the MCP result has `structuredContent`, the client drops the original text block and sends one text block with the compact JSON of `structuredContent`, after the image block.
- An MCP result with `isError: true` produced no `PostToolUse` event.

Subagent tool calls carry `agent_id` and `agent_type` and the parent's `session_id` (fixture `posttooluse-subagent-bash.json`). Routing ignores these fields. The hook keeps no state, and each invocation reads only its own stdin, so parallel tools and subagents cannot affect each other.

### Hook conflict reporting

Before it emits a replacement, the hook looks for another `PostToolUse` hook that may match the same tool. The client keeps only the last rewrite, so on a match the hook writes one line to stderr, such as `deslag: PostToolUse hook in <file> also matches Bash; leaving the output unchanged`, and exits 0 with empty stdout. The compression diagnostic from [Hook field routing](#hook-field-routing) is the other stderr line the hook can write; a rejected compression ends the hook before this check runs. With a running stats service, each `changed` result that this check blocks is recorded as `skipped` with reason `hook_conflict`, the original size emitted and the candidate fields kept as built. On `job_panicked`, the default Rust panic message (`thread ... panicked at ...`) also appears on stderr before the `deslag: compression job_panicked; tool output unchanged` line. It reads:

- user settings: `$CLAUDE_CONFIG_DIR/settings.json`, else `$HOME/.claude/settings.json`
- project settings: `.claude/settings.json` and `.claude/settings.local.json` under `$CLAUDE_PROJECT_DIR`, else under the event `cwd`
- `hooks/hooks.json` of each plugin set to `true` in `enabledPlugins` of those files, located through `<config dir>/plugins/installed_plugins.json`

An empty `CLAUDE_CONFIG_DIR`, `HOME` or `CLAUDE_PROJECT_DIR` counts as unset, so the lookup falls through to the next source.

The optional file `<state dir>/ignore-hooks` lists plugins whose hooks only observe and never return `updatedToolOutput`. Each line is a plugin key from `enabledPlugins`, for example `dev-loops@dev-loops`. Blank lines and lines that start with `#` are skipped. A `#` after a key does not start a comment, so such a line matches no plugin. The scan skips `hooks/hooks.json` of each listed plugin. Hooks in settings files are always scanned. A missing, empty or unreadable file, a file that is not a regular file and a file larger than 1 MiB change nothing.

A missing, `""` or `"*"` matcher matches every tool. A matcher made only of letters, digits, `_` and `|` matches when one `|`-separated name equals the tool name. Any other matcher is a regex. The hook does not evaluate it and treats it as matching. A hook counts as deslag's own, and is ignored, only when its whole `command` is `<deslag> hook claude`, optionally followed by `--minify-json`, or the plugin wrapper `<deslag-hook.sh>` with no arguments. The program is a bare name or a path whose last segment is `deslag` or `deslag-hook.sh`, optionally in double quotes. Any other command that mentions `deslag` still counts as a conflict. Missing, unreadable and malformed files are ignored. A file that is not a regular file or is larger than 1 MiB is ignored too.

Limits: plugins loaded with `--plugin-dir`, managed settings, hooks declared inline in `plugin.json` and `PostToolUse` hooks declared in skill or subagent frontmatter are not scanned. A conflict with these sources goes unreported. A regex matcher is treated as matching without evaluation, so it can report a false conflict. The hook cannot see which setting sources the client loaded. It reads user settings even when the client runs with `--setting-sources project,local`, so a user-level hook that the client does not run still reports a conflict (run L2). `enabledPlugins` is the union of `true` values across the files, so a project or local `false` does not override a user `true`. That case can report a false conflict, which is the safe direction.

### Hook routing live runs

Each run used a throwaway project directory, `--setting-sources project,local`, `--strict-mcp-config`, a throwaway stdio MCP server (`--mcp-config`), a throwaway plugin (`--plugin-dir`), `--no-session-persistence` and `--output-format json`. No global settings were edited.

| Run | Hook | Client | Model | Result | total_cost_usd |
| --- | --- | --- | --- | --- | --- |
| L1 | Capture script (stdin to file); three MCP tools and one subagent Bash call | 2.1.296 | claude-haiku-5-5 | MCP, structured-content and subagent payloads captured; fixtures derived. The `isError` tool produced no `PostToolUse` event | 0.00430497 |
| L2 | Debug build of `deslag hook claude --minify-json` with the real user config dir; Bash `cat` of pretty JSON and one MCP text tool | 2.1.296 | claude-haiku-5-5 | Both calls reported a conflict with a `PostToolUse` hook in the user `settings.json` on stderr and left the output unchanged | not recorded |
| L3 | As L2, with `CLAUDE_CONFIG_DIR` set to an empty directory for the hook process only | 2.1.296 | claude-haiku-5-5 | No stderr. The model quoted the minified Bash stdout `{"name":"deslag","n":[1,2]}` and the minified MCP text `{"a":1,"b":[1,2]}` | 0.00083752 |

The JSON output of run L2 was not saved, so its cost is not recorded. Its cap was USD 0.20.

### Pending

- Linux and Windows clients.
- Accepted and rejected replacements for Read and other built-in tools.
- MCP error results, resources, images and other non-text results.
- Client behavior when the hook stops reading oversize input early (the adapter drains stdin, untested live).
- Composition with other result-replacement hooks in a live client.
- One live `PostToolUse` hook run on Claude Code 2.1.296 with the mixed `hooks/hooks.json` (`"modules"` next to `"hooks"`).
- `--continue` and proxy-mode resume and fork with a live client. Hook-mode resume and fork are recorded in [ADR 0006](decisions/0006-session-identity-and-replay.md).

## Plugin distribution

`claude plugin validate` passes for the repository root and for `plugins/claude`.

### Read from the client binary

These facts were read from the Claude Code 2.1.296 client binary.

- `.claude-plugin/marketplace.json` at the repository root is the default marketplace path for a GitHub source. `/plugin marketplace add mfittko/deslag` reads it. The shorthand also accepts `owner/repo#ref`.
- A marketplace plugin entry with `source: "./plugins/claude"` points at the plugin directory. The entry carries no `version`; `plugin.json` is the only plugin version surface.
- The client substitutes `${CLAUDE_PLUGIN_ROOT}` and `${CLAUDE_PLUGIN_DATA}` in hook commands, in command markdown and in `allowed-tools`. Hook processes also receive both as environment variables.
- The data directory is `<config dir>/plugins/data/<plugin id>/`. The config dir is `CLAUDE_CONFIG_DIR` or `~/.claude`. The client creates it on demand with default permissions. `/deslag:install` sets it to mode 0700.
- `claude plugin uninstall` deletes the data directory unless `--keep-data` is passed.
- A file `commands/<name>.md` in the plugin appears as `/deslag:<name>`.

### Observed live

- Plugin command `!` injection substitutes `${CLAUDE_PLUGIN_ROOT}` and `${CLAUDE_PLUGIN_DATA}` in the command text. It does not export them as environment variables to that shell. The plugin commands therefore pass `"${CLAUDE_PLUGIN_DATA}"` to `install.sh` as an argument. `install.sh` derives the plugin root from its own location.
- `/deslag:install` installs the binary, and the hook then runs it on a Bash tool call.
- A permission rule in `allowed-tools` matches the command text after substitution. With a space in the plugin root, the rule must quote the path the same way the command does: `Bash("${CLAUDE_PLUGIN_ROOT}/scripts/install.sh":*)`.
- `/deslag:status` reports an installed and a missing binary.
- `/deslag:uninstall` removes the data directory.
- Without a binary, the hook exits 0 and the tool output is unchanged.
- `claude plugin marketplace add`, `claude plugin install` and `claude plugin uninstall` work against a local checkout. `claude plugin uninstall` removes the data directory.

The hook wrapper `plugins/claude/hooks/deslag-hook.sh` runs `$CLAUDE_PLUGIN_DATA/bin/<plugin version>/deslag hook claude`. When that file is missing, the wrapper exits 0 without output. Only `/deslag:install` downloads anything. `tests/plugin/test.sh` covers the install, checksum, hook and uninstall paths with `file://` fixtures.

### Live runs

Each run used a throwaway project directory, `--setting-sources project,local`, `--strict-mcp-config`, `--plugin-dir` (a copy of `plugins/claude`), `--no-session-persistence` and `--max-budget-usd 0.30`. A fixture release was served from a `file://` `DESLAG_RELEASE_BASE_URL`. Its fake binary only logs its arguments. No global settings were edited.

| Run | Action | Client | Model | Result | total_cost_usd |
| --- | --- | --- | --- | --- | --- |
| 1 | `/deslag:install` before the data dir argument fix | 2.1.296 | claude-haiku-5-5 | Failed with "CLAUDE_PLUGIN_ROOT is not set". The client does not export plugin variables to `!` shells | 0 |
| 2 | `/deslag:install` | 2.1.296 | claude-haiku-5-5 | "Installed deslag 0.1.0 for aarch64-apple-darwin at `<data>/bin/0.1.0/deslag`" | 0.00147325 |
| 3 | Bash tool call `echo hello-deslag` with the binary installed | 2.1.296 | claude-haiku-5-5 | The fake binary logged `hook claude`; model saw `hello-deslag` | 0.00117793 |
| 4 | `/deslag:status` | 2.1.296 | claude-haiku-5-5 | Installed | 0.00096749 |
| 5 | `/deslag:uninstall` | 2.1.296 | claude-haiku-5-5 | Removed the data directory | 0.00097209 |
| 6 | `/deslag:status` | 2.1.296 | claude-haiku-5-5 | Missing, with the fix hint | 0.00096049 |
| 7 | Bash tool call with no binary | 2.1.296 | claude-haiku-5-5 | Hook finished with status 0 (6 ms); model saw `hello-deslag` | 0.00042008 |
| 8 | `/deslag:status`, plugin copy at a path with a space, quoted command and unquoted `allowed-tools` rule | 2.1.296 | claude-haiku-5-5 | Refused: "Shell command permission check failed ... This command requires approval" | 0 |
| 9 | `/deslag:status`, plugin path without a space, quoted command and unquoted rule | 2.1.296 | claude-haiku-5-5 | Missing, with the fix hint | 0.00019403 |
| 10-11 | `/deslag:status` with the quoted rule `Bash("${CLAUDE_PLUGIN_ROOT}/scripts/install.sh":*)`, with and without a space in the path | 2.1.296 | claude-haiku-5-5 | Missing, with the fix hint, in both runs | 0.00038806 |
| 12 | `/deslag:install`, path with a space | 2.1.296 | claude-haiku-5-5 | Installed at `<data>/bin/0.1.0/deslag` | 0.00020576 |
| 13 | `/deslag:status` | 2.1.296 | claude-haiku-5-5 | Installed | 0.00019913 |
| 14 | Bash tool call `echo hello-deslag` | 2.1.296 | claude-haiku-5-5 | The fake binary logged `hook claude`; model saw `hello-deslag` | 0.00039158 |
| 15 | `/deslag:uninstall` | 2.1.296 | claude-haiku-5-5 | Removed the data directory | 0.00021261 |
| 16 | `/deslag:status` | 2.1.296 | claude-haiku-5-5 | Missing, with the fix hint | 0.00019403 |
| 17 | `/deslag:install` against the published `v0.1.0-alpha.1` release (default `https` base URL, plugin version set to `0.1.0-alpha.1` in the copy) | 2.1.296 | claude-haiku-5-5 | Installed the real arm64 binary at `<data>/bin/0.1.0-alpha.1/deslag`; data and bin dirs mode 0700 | 0.00098499 |
| 18 | Bash tool call `echo hello-deslag` with the real binary installed | 2.1.296 | claude-haiku-5-5 | Hook finished with status 0 (409 ms, first launch); model saw `hello-deslag` | 0.00039158 |
| 19 | `/deslag:uninstall` | 2.1.296 | claude-haiku-5-5 | Removed the data directory | 0.00020361 |

Total: USD 0.00933671. Runs 1-7 used the scripts before the pre-PR review fixes. Runs 12-16 used the scripts at commit 9e498e83e100a6e3216364740e182dc743e9dd16. Runs 17-19 used the scripts at commit eb1708138a311bcd8eec4dd94a81c4d03a72179f. The refusals of a symlinked bin dir, version dir, binary path and marker are covered by `tests/plugin/test.sh` only.

Runs 1-16 and the CLI runs below used plugin version 0.1.0. They predate the v0.1.0-alpha.1 bump (468d3dbbc7a936991f7a5c25f6dee632c6e42d34), which reached this branch through merge 961a45e865c9362e30cd4b2e3474fd98c49f1087. Run 17 used commit eb1708138a311bcd8eec4dd94a81c4d03a72179f, which also predates that merge, so the copy needed the version override. The exact commit under test in the CLI runs was not recorded. They were recorded in commits 8b0b073048fc96315de77713860cdccab14c73a1 (local marketplace) and b4123b0bfffbd51c17cb533664cc04dc35a134bf (GitHub source at the branch ref).

These zero-cost CLI runs used an isolated `CLAUDE_CONFIG_DIR`. No global settings were touched.

- `claude plugin marketplace add <worktree path>` added the marketplace `deslag`.
- `claude plugin install deslag@deslag` installed version 0.1.0.
- After `install.sh` seeded `plugins/data/deslag-deslag`, `claude plugin uninstall deslag@deslag` removed that data directory.
- `claude plugin marketplace add mfittko/deslag#issue-40` (the GitHub source at the pull request branch) added the marketplace. `claude plugin install deslag@deslag` installed version 0.1.0. The cached plugin holds the plugin manifest `.claude-plugin/plugin.json` plus the `commands`, `hooks` and `scripts` directories.

The pre-PR review fixes between runs 7 and 12 cover symlinked version dirs, the cleanup trap, quoted command paths, download timeouts, status detection, scoped uninstall and the missing-subcommand usage message.

### Pending

- `/plugin marketplace add mfittko/deslag` against `main`. This needs the merge. The same GitHub source at the branch ref `mfittko/deslag#issue-40` passed. That branch-ref run is the accepted pre-merge evidence for the acceptance criterion "`/plugin marketplace add mfittko/deslag` lists the plugin". The post-merge run against `main` completes it.

## Claude Code usage module

The usage module `plugins/claude/hooks/usage.js` observes `turn.step` and sends each model request's provider usage to the local service. See [Observability](observability.md#usage-module). It loads only with `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1` and sends only with `DESLAG_USAGE_STATS=1`.

### Verified offline

These facts come from `claude plugin test plugins/claude` and `claude plugin validate plugins/claude` on Claude Code 2.1.296 and 2.1.299. Two bullets also cite live checks: the `ui.status` bullet points to the demo recorded run, and the mod API declarations bullet points to run U1 below.

- `hooks/hooks.json` may hold `"modules"` next to `"hooks"`. Validation passes and lists the module's `turn.step` hook on 2.1.296 and 2.1.299. `claude plugin test plugins/claude` passed 32 of 32 on 2.1.296 and 2.1.299 with the suite before the compression toggle. The current suite passes 40 of 40 on 2.1.302. Clients 2.1.297 and 2.1.298 are untested offline with it.
- The savings widget uses the host APIs `$.ui.status` and `$.clock.sleep`. The widget tests in `plugins/claude/tests/usage.test.ts` hook `ui.status` and `clock.sleep` and pass on 2.1.296 and 2.1.299. It also reads the compression toggle and the owner with `$.fs.read`; the tests hook `fs.read` and pass on 2.1.302. The [demo recorded run](demo.md#recorded-run) is the live check of `ui.status` on 2.1.299.
- The observer passes every chunk and the final result through unchanged, with the module on and off.
- A `$.http.fetch` started after the `turn.step` generator returned still runs. Its `socketPath` option reaches a Unix stream socket.
- The mod API declarations (written by Claude Code 2.1.277) type the `turn.step` result's `usage` as `input_tokens`, `output_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens` and `model`, or `null`. Run U1 below confirms the four token fields and `model` live.

### Live capture

| Run | Client | Command | Model | Observed `usage` | total_cost_usd |
| --- | --- | --- | --- | --- | --- |
| U1 | 2.1.299 | `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 DESLAG_USAGE_STATS=1 claude -p "Run the Bash command: cat data.json ..." --model haiku --allowedTools Bash` with a copy of `plugins/claude` whose hook runs `deslag hook claude --minify-json`, owner `hook`, `deslag local start` | claude-haiku-5-5 | Two steps of one turn, each with all four token fields and `model` `claude-haiku-5-5`: step 0 `input_tokens` 2, `cache_read_input_tokens` 11,447, `cache_creation_input_tokens` 6,426, `output_tokens` 79; step 1 `input_tokens` 2, `cache_read_input_tokens` 17,873, `cache_creation_input_tokens` 2,303, `output_tokens` 3. `deslag stats --json --session <id>` listed both requests, no drops. The one hook report (a 4,105-byte pretty JSON file) went to step 1 with 723 estimated tokens removed | 0.0020804 |

Status: live verified for the usage fields, the HTTP bridge and the per-request attribution. The `null` usage of a step without a response was not produced live; it stays documented and is covered by tests. The run used a throwaway project directory, `--setting-sources project,local`, `--strict-mcp-config`, `--plugin-dir`, `--no-session-persistence`, `--max-budget-usd 0.30`, `--output-format json` and a throwaway `DESLAG_STATE_DIR`. No global settings were edited.
