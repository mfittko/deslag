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
- The client binary documents that hooks run in parallel on the original output and that rewrites from several hooks are last-write-wins. This was read from the client, not tested.

### Live runs

Each run used a throwaway project directory, `--setting-sources project,local`, `--strict-mcp-config`, `--plugin-dir` (a throwaway plugin for runs 1-3, a copy of `plugins/claude` for run 4), `--no-session-persistence`, `--max-budget-usd 0.40` and `--output-format json`. No global settings were edited.

| Run | Hook | Client | Model | Result | total_cost_usd |
| --- | --- | --- | --- | --- | --- |
| 1 | Capture script (stdin to file), Bash and Read | 2.1.296 | claude-haiku-5-5 | Payloads captured; fixtures derived | 0.00182775 |
| 2 | Valid Bash replacement (`stdout` set to `REPLACED-BY-HOOK`, other keys kept) | 2.1.296 | claude-haiku-5-5 | Model saw `REPLACED-BY-HOOK`; client logged "replaced tool output" | 0.00121455 |
| 3 | Invalid Bash replacement (bare string) | 2.1.296 | claude-haiku-5-5 | Model saw original `hello-deslag`; client logged the shape mismatch | 0.00042 |
| 4 | `deslag hook claude` through `plugins/claude` | 2.1.296 | claude-haiku-5-5 | Hook ran (`finished with status 0`); output unchanged | 0.0003915 |

Total: USD 0.0038538. The shared M0 live-run budget is USD 5; the operator allotted USD 2.50 of it to this item.

Runs 2 and 3 used throwaway `jq` hooks to characterize the client. The deslag adapter does not emit replacements yet. Its `replacement` helper applies the same shape rule, and unit tests cover the accepted and rejected cases.

### Pending

- Linux and Windows clients.
- Accepted and rejected replacements for Read and other built-in tools.
- MCP tool results, images and other non-text results.
- Client behavior when the hook stops reading oversize input early (the adapter drains stdin, untested live).
- Composition with other result-replacement hooks.
- Resume and fork behavior of replaced results (DF-004).
