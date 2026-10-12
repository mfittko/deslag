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

### Observed contract

- The client sends one JSON object on stdin. Captured Bash and Read payloads are in `tests/fixtures/claude/` (paths and IDs replaced with placeholders).
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

`deslag hook claude --minify-json` and `deslag proxy --minify-json` remove insignificant whitespace from JSON in Bash tool results only. Both call `deslag_core::compress` with `FIRST_SLICE_JSON_MINIFY_CONTEXT` and commit the output only for a `Changed` result, through `into_changed`. The plugin hook in `plugins/claude` does not pass the flag, so it stays a no-op. Neither flag has been run against a live client. Token savings from whitespace removal have not been measured.

Status: experimental. Each flag compresses only when the configured owner in `<state dir>/owner` names its component, so the hook and the proxy never both transform one session. The proxy pins per-result decisions in a session manifest and replays them; see the README section on the compression owner and proxy replay.

- Removing the proxy or its `--minify-json` flag mid-session makes the client resend original results. Content stays correct, but the cached prefix is lost. Start a new session.
- The proxy passes requests with `content-encoding` through unchanged. A client that compresses only some requests would see drift.
- The proxy passes requests with a body-integrity header (`content-digest`, `digest`, `content-md5`, `x-amz-content-sha256`, `repr-digest`) through unchanged.
- The proxy passes a request with trailers through unchanged. Its trailers are not forwarded upstream. A client that sends trailers on only some requests would see drift.
- The proxy does not rewrite `/v1/messages/count_tokens`. Its token counts can overstate the rewritten request.
- The proxy buffers and rewrites at most 4 eligible requests at once (`INSPECT_PERMITS`). Others wait for a permit. A body over 32 MiB (`MAX_INSPECT_BYTES`) is forwarded unrewritten. A buffered body that does not arrive within `BODY_READ_TIMEOUT` (300 s) of its permit fails with `408`. The deadline covers only the first 32 MiB. For a larger body the permit is released and the rest streams without a deadline. See the README proxy limits.

### Pending

- Linux and Windows clients.
- Accepted and rejected replacements for Read and other built-in tools.
- MCP tool results, images and other non-text results.
- Client behavior when the hook stops reading oversize input early (the adapter drains stdin, untested live).
- Composition with other result-replacement hooks.
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
