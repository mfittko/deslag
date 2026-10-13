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

Reproduce:

```sh
cargo test --locked -p deslag-cli codec
cargo bench --locked -p deslag-cli --bench mod_bridge
claude plugin validate plugins/claude-mod-experimental
claude plugin test plugins/claude-mod-experimental
```

To load the mod in a local session (live, not done in this spike): build `deslag`, write `mod` to `$DESLAG_STATE_DIR/owner` (default `~/.local/state/deslag/owner`), and start `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 DESLAG_MOD_MODE=observe DESLAG_MOD_BINARY=/abs/path/to/deslag claude --plugin-dir plugins/claude-mod-experimental`. Use `safe` instead of `observe` to rewrite rows.

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

Status values: `documented` (public declarations or docs only), `offline verified` (this repository's tests or the installed engine under `claude plugin test`, no model), `live verified` (a real session with a model), `pending`.

### Evidence ledger

| # | Claim | Source and command | Status | Limitations |
| --- | --- | --- | --- | --- |
| E1 | A `session.append` hook with matcher `{ door: "tool-result" }` loads on 2.1.296 and its rewrite before `next` is the row the engine stores | `claude plugin test plugins/claude-mod-experimental`, test "safe mode stores the codec replacement"; 18 of 18 tests pass | offline verified | The test drives the engine's chain directly. It does not prove the model reads the row; see E14 |
| E2 | The rewrite keeps `tool_use_id`, the other blocks and their order; media blocks stay in place | Same suite, "media blocks keep their place" | offline verified | Declarations say the engine also restores pinned fields itself; not exercised |
| E3 | `$.process.run` can be answered and refused beneath a mod. A refusal, nonzero exit, cut stdout, wrong UTF-8 byte count, missing or extra slot, or `complete` not `true` stores the original, and `next` runs exactly once | Same suite, seven "the original is stored once" cases | offline verified | The real output limit and timeout behavior are not reproduced offline |
| E4 | One failed slot leaves the whole row unchanged | Same suite | offline verified | |
| E5 | Another rewriting mod beneath deslag changes the stored row after deslag's proposal: stored `OTHER(MIN(x))`, codec input `x`. Only a comparison with the stored row can tell a delivered replacement from an overridden one | Same suite, "composition"; the mod counts `overridden` when the stored text differs | offline verified | Order among user-tier mods follows load order here; not a documented guarantee |
| E6 | The `turn.step` observer forwards every chunk and the final result unchanged | Same suite, "turn.step observer" | offline verified | On 2.1.296, `stream.result` settled `undefined` after a `for await`; the generator's own return value was correct |
| E7 | The engine's loader statically checks a hooks module: a hook must be a function literal or the name of one, `$` must be spelled `$.noun.event(...)` at the call site, and an inline test plugin cannot call an imported function | `claude plugin validate`, `claude plugin test` errors on the first draft | offline verified | Shapes the code: hooks are top-level named functions; configuration lives in module state |
| E8 | `claude plugin validate` lists each module's hooks, the environment names it reads, and `calls: $.process.run`, and warns "gating hook without .catch" for the `session.append` hook | `claude plugin validate plugins/claude-mod-experimental` | offline verified | The warning is advisory; the hook catches bridge errors and lets a `next` failure propagate on purpose |
| E9 | Model-visible Bash content is `stdout` when `stderr` is empty and `stdout + stderr` when it is not. Of 3,360 stored Bash rows in local transcripts from the last 5 days: 2,887 equal `stdout`, 334 equal `stdout + stderr`, 89 are `(Bash completed with no output)`, 50 match none of these | Shape-only `jq` aggregation over `~/.claude/projects/**/*.jsonl` (counts, no content read) | offline verified | The 50 unmatched rows were not inspected: reading their content was denied by the session's permission classifier. Large-output previews (E10) are the likely cause |
| E10 | The client replaces a large Bash result with a preview and a file path before the model reads it. In this spike's own session, a 31.5 KB result arrived as "Output too large (31.5KB). Full output saved to ..." plus a 2 KB preview | Observed in this session's tool output; the binary contains the string `Output too large (` | offline verified | Whether `session.append` sees the full text or the preview, and whether the PostToolUse hook runs before this cut, is unknown. This decides how much a mod can compress; see Q1 |
| E11 | `deslag codec --protocol 1` returns one complete envelope, keeps number lexemes and UTF-8 byte counts exact, reports observe mode as `would_change` with emitted bytes equal to the original, and in safe mode changes text only when the owner file reads `mod` | `cargo test --locked -p deslag-cli codec` (10 tests) | offline verified | |
| E12 | Owner `mod` makes the hook and the proxy stand down without a change to either: `read_owner` maps any value other than `hook`/`proxy` to `none`, the hook compresses only under `hook`, and the proxy decides new results only under `proxy` while it still replays pinned decisions | Code reading of `crates/deslag-proxy/src/session.rs` and `crates/deslag-cli/src/main.rs`; existing test `no_routed_field_is_replaced_unless_the_hook_owns_compression` | offline verified | No end-to-end test with a live hook and mod in one session |
| E13 | Bridge cost is at or below the hook's at every size (table below), and every predeclared native budget passes | `cargo bench --locked -p deslag-cli --bench mod_bridge` | offline verified | Excludes the client's `$.process.run` dispatch; see Q3 |
| E14 | The model reads the rewritten row, and the transcript file never holds the pre-rewrite form of `content` | Declarations at `bf2323b` (`session.append` doc comment) | documented | Live sentinel probe pending (Q2) |
| E15 | The transcript keeps the tool's structured record (`toolUseResult`) as made, so the original output stays on disk under a mod. The hook replaced that record too ([ADR 0006](../decisions/0006-session-identity-and-replay.md), run A) | Declarations: `SessionAppendInput` "Not on `e`, so stored as made: a tool result's structured record" | documented | Live transcript inspection pending. A mod is therefore not a way to keep large originals off disk |
| E16 | Stored rewrites survive resume, continue, fork, model switch, disable, reload and upgrade | None for the mod; [ADR 0006](../decisions/0006-session-identity-and-replay.md) covers the hook | pending | Expected to match hook mode, because the client stores the row; unproven |
| E17 | `$.process.run` takes argv and `{ cwd, env, stdin, timeoutMs }`; the timeout is 30 s by default and 10 min at most; stdout and stderr are "cut at the output limit" with no truncation flag; it has no abort-signal parameter; it is "CLI only" | Declarations at `bf2323b` | documented | The cut makes the envelope check in E3 mandatory. Abandoned native work runs until its own `BoundedExecutor` budget (10 ms per job) ends. Desktop and other surfaces get no compression |
| E18 | `turn.step` results carry `usage: { input_tokens, output_tokens, cache_read_input_tokens, cache_creation_input_tokens, model } \| null`; `null` when no response arrived | Declarations at `bf2323b`; the observer counts `null` as `unknown` (unit test) | documented, offline verified for the counting | Live usage capture pending |
| E19 | `$.telemetry` refuses user-installed and administrator-installed mods | Built-in telemetry README at `bf2323b` | documented | Deslag keeps its own bounded pipeline |

### Benchmark: hook versus codec bridge

Apple M5 Max, macOS, rustc 1.96.0, release build of this branch, concurrency 1, whole process (spawn, pipes, exit). Payloads are pretty JSON records generated by `crates/deslag-cli/benches/mod_bridge.rs`. The client's `$.process.run` dispatch is not included.

| Payload | Path | Samples | p50 | p95 | p99 | stdout bytes | Exit 0 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 2 KiB | hook `--minify-json` | 200 | 1.52 ms | 1.85 ms | 1.97 ms | 1,716 | yes |
| 2 KiB | codec safe | 200 | 1.34 ms | 1.45 ms | 1.52 ms | 2,072 | yes |
| 2 KiB | codec observe | 200 | 1.32 ms | 1.42 ms | 1.50 ms | 533 | yes |
| 64 KiB | hook `--minify-json` | 200 | 1.72 ms | 1.81 ms | 1.87 ms | 50,030 | yes |
| 64 KiB | codec safe | 200 | 1.62 ms | 1.74 ms | 1.91 ms | 50,391 | yes |
| 64 KiB | codec observe | 200 | 1.55 ms | 1.68 ms | 1.79 ms | 537 | yes |
| 1 MiB | hook `--minify-json` | 50 | 7.46 ms | 7.78 ms | 7.86 ms | 807,458 | yes |
| 1 MiB | codec safe | 50 | 6.00 ms | 6.28 ms | 6.36 ms | 807,826 | yes |
| 1 MiB | codec observe | 50 | 4.82 ms | 4.92 ms | 5.09 ms | 544 | yes |
| 17 MiB | hook `--minify-json` | 5 | 5.03 ms | 5.14 ms | 5.14 ms | 0 | yes (passes through) |
| 17 MiB | codec safe | 5 | 4.97 ms | 5.09 ms | 5.09 ms | 0 | no (exit 65, original kept) |
| 17 MiB | codec observe | 5 | 4.96 ms | 5.06 ms | 5.06 ms | 0 | no (exit 65, original kept) |

Budget check: 64 KiB codec p95 1.74 ms against hook 1.81 ms and the 25 ms ceiling, pass. 1 MiB codec p95 6.28 ms against 100 ms, pass. Above the cap: exit 65, no stdout, p95 5.09 ms against 250 ms, pass. Client dispatch overhead: pending. Memory and allocations were not measured.

The 2 KiB safe envelope is larger than its input because the report travels with the replacement. A bridge for large payloads should drop the report from the transport or keep it as the only content of observe responses.

### Comparison

| Area | PostToolUse hook (shipped) | Mod `session.append` (experimental) | Proxy |
| --- | --- | --- | --- |
| Rewrite target | Structured `tool_response` (Bash `stdout` only, MCP text blocks) | Model-visible `content`; Bash `stdout` and `stderr` arrive joined | Request bytes on the wire |
| Bash with stderr | Compresses `stdout`, keeps `stderr` | Whole content must parse as one JSON document, so these rows pass through (about 10 % of Bash rows in E9) | Same as mod |
| History | Client stores the replacement (live, ADR 0006) | Client stores the row (documented, pending live) | Replay manifests (offline) |
| Original on disk | Removed from the transcript (live) | Kept in `toolUseResult` (documented) | Kept (client stores originals) |
| Composition | Last writer wins; deslag scans settings for conflicts | Middleware chain; stored-row comparison detects overrides (offline) | Separate process |
| Native bridge | Client spawns the binary per tool result | Mod spawns the binary per row via `$.process.run` | Long-running process |
| Usage observation | None | `turn.step` usage per step (documented) | Response usage (planned) |
| Surfaces | Settings hooks | CLI only for `$.process.run` | Any client that honors `ANTHROPIC_BASE_URL` |
| Install | Plugin plus verified binary | Same plugin, plus `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1` while Mods are early access | Launcher and credentials routing |

### Instrumentation placement

Option A (one installable mod, internal modules) is prototyped: the usage observer registers regardless of compression mode, so it works with compression off. Option B (a second installable mod) needs a versioned inter-mod contract. Two mods share no module state (E7); the only shared channels are events and `$.store`, a persistent key-value store that is neither a queue nor a safe concurrent counter. Nothing in this spike shows utility for a separately installed observer that Option A lacks.

### Open questions for live verification

- Q1. Does `session.append` see the full Bash text or the large-output preview (E10)? Does the PostToolUse hook run before the cut? If the mod sees only previews, the mod cannot compress the results that matter most.
- Q2. Sentinel probe: does the model read the rewritten row, and does a fresh client process read it after `--resume`, `--continue` and `--fork-session` with the mod disabled (E14, E16)?
- Q3. What is the real `$.process.run` output limit and dispatch overhead, and what happens to a child that outlives `timeoutMs` (E17)?
- Q4. Where does a mod sit relative to the classic PostToolUse hook, and can a mod observe hook outcomes through the `classic.PostToolUse` event?
- Q5. Linux x86_64, headless (`-p`) and SDK surfaces.

Suggested live budget: the ADR 0006 runs cost USD 0.019 for five calls. A cap of USD 1.00 total and USD 0.30 per call on `claude-haiku-5-5` covers Q1 to Q4.

## Recommendation

Verdict: conditional. Recommend the hybrid path now and mod-first Claude compression only after the live gates Q1 to Q3 pass. Put instrumentation in the same mod as a separate internal module (Option A). [ADR 0007](../decisions/0007-claude-mod-first-integration.md) records the proposal.

Reasons:

- The engine-level mechanics work on the installed client (E1 to E8), and the native bridge is no slower than the hook (E13).
- Two facts limit the mod's compression value until live evidence exists: Bash rows with stderr pass through (E9), and large results may already be previews at the boundary (E10, Q1).
- The mod does not remove originals from disk (E15), and `$.process.run` is CLI only (E17).
- The usage observer gives deslag provider-reported usage for the first time without a proxy (E6, E18). This is independent of compression ownership.

Staged plan. Each step is one small PR with its own tests; none deletes the proxy or changes a release promise.

| Step | Change | Depends on | Acceptance | Rollback |
| --- | --- | --- | --- | --- |
| 1 | This spike: codec bridge, experimental mod, benchmark, ADR (proposed) | none | Tests above pass; ADR accepted or rejected by the owner | Revert the PR; nothing ships |
| 2 | Live verification of Q1 to Q4 with operator-authorized caps; record results in `docs/compatibility.md` | 1, budget authorization | Sentinel probe and lifecycle table filled for the mod | Docs only |
| 3 | `Owner::Mod` in `deslag_proxy::session` and the owner writer in the #20 launcher/doctor work; the hook conflict scan reports an active mod | 2 | Owner file round trip; hook and proxy stand down under `mod` (tests) | Owner back to `hook` |
| 4 | Usage observer and compression counters in `plugins/claude` as an opt-in mod module, feeding the #21 local service over a bounded bridge | 2, #21 | Works with compression off; drops are counted; no awaited I/O in the hot path | Disable the mod module; the hook is unaffected |
| 5 | Mod compression as an opt-in alternative to the hook, binary path from the verified install, one owner selected through the owner file | 2 passing Q1 to Q3, 3 | #22 and #23 gates extended to the mod adapter | Owner `hook`, disable the mod module |
| 6 | Decide the Claude default (hook or mod) and whether the proxy requirement leaves the Claude MVP | 5, a paired evaluation run | Owner decision recorded in a new ADR | Keep the hook default |

Issue impact (proposals, not edits to those issues):

- #18 / PR #72: merged. Manifests stay for the proxy; the mod needs no replay state because the client stores the row.
- #19: no change until step 5. Mod compression is not a reason to declare proxy acceptance criteria met.
- #20: split generic config and diagnostics from proxy launch; add the owner writer for `mod`.
- #21: reuse the report schema and bounded delivery for mod counters.
- #22 / #23: add mod adapter compatibility, fault, security, performance and clean-install gates at step 5; codec and model-quality gates stay.
- #4, #77, #84: no impact on `v0.1.0-alpha.3` or the frozen evaluation.
- Future OTLP: the exporter stays outside the mod and outside the core; default exports stay metadata-only.

Session boundaries for owner changes: hook to mod and mod to hook are safe at any time, because the client stores both forms. Proxy to mod keeps working while the proxy stays in the path, since it replays pinned decisions under any owner. Removing the proxy from a session with encoded history needs a new session (ADR 0006).

Spike exit: graduate after the owner accepts ADR 0007; the operator runs `exit-spike.mjs --disposition graduate` with a plan file for step 2.
