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

`deslag hook claude --minify-json` and `deslag-proxy --minify-json` remove insignificant whitespace from JSON in Bash tool results only. Both call `deslag_core::minify_json`. The plugin hook in `plugins/claude` does not pass the flag, so it stays a no-op. Neither flag has been run against a live client. Whitespace removal changes bytes; no token saving is claimed.

Status: experimental. The proxy rewrite is stateless and keeps no replay manifest yet.

- Enabling, disabling or upgrading `--minify-json` mid-session rewrites already-submitted history on resend. Start a new session after any such change.
- Do not enable the hook flag and the proxy flag at the same time. One component owns compression.
- The proxy passes requests with `content-encoding` through unchanged. A client that compresses only some requests would see drift.
- The proxy passes requests with a body-integrity header (`content-digest`, `digest`, `content-md5`, `x-amz-content-sha256`, `repr-digest`) through unchanged.
- The proxy does not rewrite `/v1/messages/count_tokens`. Its token counts can overstate the rewritten request.

### Pending

- Linux and Windows clients.
- Accepted and rejected replacements for Read and other built-in tools.
- MCP tool results, images and other non-text results.
- Client behavior when the hook stops reading oversize input early (the adapter drains stdin, untested live).
- Composition with other result-replacement hooks.
- `--continue` and proxy-mode resume and fork with a live client. Hook-mode resume and fork are recorded in [ADR 0006](decisions/0006-session-identity-and-replay.md).
