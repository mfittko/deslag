# Spike: Claude Code mod-first integration and instrumentation placement

## Question

Should Claude Code Mods (session.append plus a native Rust bridge) become deslag's primary Claude integration instead of the PostToolUse settings hook and the Anthropic HTTP proxy, and should instrumentation live in a module of the same mod or in a separately installable mod? (issue #86)

## Approach

Frozen inputs:

| Input | Value |
| --- | --- |
| Repository | `main` at `f1ab0d6` (the issue named `368a619`; PR #72 merged in between) |
| Installed client | Claude Code 2.1.296, macOS arm64 (`/Users/mfittko/.local/share/claude/versions/2.1.296`) |
| Public declarations | `anthropics/claude-code` `bf2323b`, `mods/types/claude-code.d.ts`, blob `e3636ad`, header "Written by Claude Code 2.1.277" |
| Installed declarations | Not available. 2.1.296 has no `/plugin-types` command, and the binary carries no readable declaration text. The installed engine was exercised directly through `claude plugin validate` and `claude plugin test` instead |
| Function hooks | `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1` in user settings; the managed policy (`policy-limits.json`) restricts nothing related to plugins or mods |
| Codec policy | `json-minify` v1, `FIRST_SLICE_JSON_MINIFY_CONTEXT`, no new codec enabled |

Method:

1. Add a narrow bridge, `deslag codec --protocol 1`, to the existing CLI. It reuses the hook's minify job and `BoundedExecutor`. No core change.
2. Build an experimental mod in `plugins/claude-mod-experimental/`. It is not listed in `.claude-plugin/marketplace.json`, its default mode is `off`, and it needs an absolute binary path. It has one entrypoint with two internal modules: a `session.append` compression adapter and a `turn.step` usage observer.
3. Test the mod against the installed engine with `claude plugin test`. The engine runs its own `session.append` and `turn.step` chains; the test answers `process.run` beneath the mod. No binary, network or model is involved.
4. Measure the native side with a whole-process benchmark against the existing hook, on the same payloads.
5. Read stored tool-result shapes from local 2.1.296 transcripts as aggregates (shape counts only, no content).
6. No paid model call was made. Every live check is recorded as pending.

### Predeclared budgets

Declared before the benchmark ran. They follow PLAN §13.2 where it has a case and are new otherwise.

| Case | Budget |
| --- | --- |
| Codec bridge, whole process, payload up to 64 KiB | p95 within 2 ms of the hook's p95 on the same payload, and p95 at most 25 ms (PLAN §13.2 warm hook) |
| Codec bridge, 1 MiB payload | Completes with a complete envelope; p95 at most 100 ms |
| Payload above the 16 MiB cap | Exit nonzero, no stdout, original kept; p95 at most 250 ms |
| Client `$.process.run` overhead over the native call | p95 at most 10 ms (pending live) |
| Instrumentation in the hot path | No awaited I/O; counters only (PLAN §12.2) |

## Findings

TBD — filled in during the spike.
