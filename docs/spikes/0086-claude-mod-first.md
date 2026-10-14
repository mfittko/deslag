# Spike: Claude Code mod-first integration and instrumentation placement

**Status:** historical evidence record. [ADR 0008](../decisions/0008-mod-instrumentation-only.md) drops mod compression and removes the codec bridge; the mod is for instrumentation only.

## Question

Should Claude Code Mods (session.append plus a native Rust bridge) become deslag's primary Claude integration instead of the PostToolUse settings hook and the Anthropic HTTP proxy, and should instrumentation live in a module of the same mod or in a separately installable mod? (issue #86)

## Approach

Frozen inputs:

| Input | Value |
| --- | --- |
| Repository | `main` at `f1ab0d6` (the issue named `368a619`; PR #72 merged in between) |
| Installed client | Claude Code 2.1.296, macOS arm64, for the offline work. The client auto-updated to 2.1.299 during the spike. Every live run used 2.1.299, and the offline engine suite passes on both versions |
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
6. Live checks Q1 to Q3 ran on 2.1.299 under the operator-authorized caps (USD 1.00 total, USD 0.30 per call, haiku only). See [Live verification](#live-verification). Other live checks stay pending.

Reproduce:

```sh
cargo test --locked -p deslag-cli codec
cargo bench --locked -p deslag-cli --bench mod_bridge
claude plugin validate plugins/claude-mod-experimental
claude plugin test plugins/claude-mod-experimental
```

To load the mod in a local session (as in live runs A, L and E; see [Live verification](#live-verification)): build `deslag`, write `mod` to the `owner` file in the deslag state directory (`$DESLAG_STATE_DIR`, else `$XDG_STATE_HOME/deslag`, else `~/.local/state/deslag`; see `state_dir` in [deslag-telemetry](../../crates/deslag-telemetry/src/lib.rs)), and start `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 DESLAG_MOD_MODE=observe DESLAG_MOD_BINARY=/abs/path/to/deslag claude --plugin-dir plugins/claude-mod-experimental`. Use `safe` instead of `observe` to rewrite rows.

### Predeclared budgets

Declared before the benchmark ran. They follow [PLAN §13.2](../PLAN.md#132-proposed-initial-budgets) where it has a case and are new otherwise.

| Case | Budget |
| --- | --- |
| Codec bridge, whole process, payload up to 64 KiB | p95 within 2 ms of the hook's p95 on the same payload, and p95 at most 25 ms ([PLAN §13.2](../PLAN.md#132-proposed-initial-budgets) warm hook) |
| Codec bridge, 1 MiB payload | Completes with a complete envelope; p95 at most 100 ms |
| Payload above the 16 MiB cap | Exit nonzero, no stdout, original kept; p95 at most 250 ms |
| Client `$.process.run` overhead over the native call | p95 at most 10 ms (measured live in E21) |
| Instrumentation in the hot path | No awaited I/O; counters only ([PLAN §12.2](../PLAN.md#122-hot-path-contract)) |

## Findings

Status values: `documented` (public declarations or docs only), `offline verified` (this repository's tests or the installed engine under `claude plugin test`, no model), `observed` (read from the local client's own artifacts or this session's tool output, without a controlled probe), `live verified` (a controlled session with a model), `pending`.

### Evidence ledger

| # | Claim | Source and command | Status | Limitations |
| --- | --- | --- | --- | --- |
| E1 | A `session.append` hook with matcher `{ door: "tool-result" }` loads on 2.1.296 and its rewrite before `next` is the row the engine stores | `claude plugin test plugins/claude-mod-experimental`, test "safe mode stores the codec replacement"; the whole suite passes | offline verified | The test drives the engine's chain directly. It does not prove the model reads the row; see E14 |
| E2 | The rewrite keeps `tool_use_id`, the other blocks and their order; media blocks stay in place | Same suite, "media blocks keep their place" | offline verified | Declarations say the engine also restores pinned fields itself; not exercised |
| E3 | `$.process.run` can be answered and refused beneath a mod. A refusal, nonzero exit, cut stdout, wrong UTF-8 byte count, missing or extra slot, or `complete` not `true`, a mismatched policy label, or an unexpected row shape stores the original, and `next` runs exactly once | Same suite, eight "the original is stored once" cases and "unexpected row shapes" | offline verified | Mocked responses only. The real output limit and timeout behavior are not reproduced offline (Q3) |
| E4 | One failed slot leaves the whole row unchanged | Same suite | offline verified | |
| E5 | Another rewriting mod beneath deslag changes the stored row after deslag's proposal: stored `OTHER(MIN(x))`, codec input `x`. Only a comparison with the stored row can tell a delivered replacement from an overridden one | Same suite, "composition"; the mod counts `overridden` when the stored text differs | offline verified | The order `OTHER(MIN(x))` was observed once on 2.1.296 and is not asserted; the test accepts either order. Order among user-tier mods is not a documented guarantee |
| E6 | The `turn.step` observer forwards every chunk and the final result unchanged | Same suite, "turn.step observer" | offline verified | On 2.1.296, `stream.result` settled `undefined` after a `for await`; the generator's own return value was correct |
| E7 | The engine's loader statically checks a hooks module: a hook must be a function literal or the name of one, `$` must be spelled `$.noun.event(...)` at the call site, and an inline test plugin cannot call an imported function | `claude plugin validate`, `claude plugin test` errors on the first draft | offline verified | Shapes the code: hooks are top-level named functions; configuration lives in module state |
| E8 | `claude plugin validate` lists each module's hooks, the environment names it reads, and `calls: $.process.run`, and warns "gating hook without .catch" for the `session.append` hook | `claude plugin validate plugins/claude-mod-experimental` | offline verified | The warning is advisory. The hook catches every preparation and bridge error and lets a `next` failure propagate on purpose, so a failed store is never retried |
| E9 | Model-visible Bash content is `stdout` when `stderr` is empty and `stdout + stderr` when it is not. Of 3,360 stored Bash rows in local transcripts from the last 5 days: 2,887 equal `stdout`, 334 equal `stdout + stderr`, 89 are `(Bash completed with no output)`, 50 match none of these | Shape-only `jq` aggregation over `~/.claude/projects/**/*.jsonl` (counts, no content read) | observed | The 50 unmatched rows were not inspected: reading their content was denied by the session's permission classifier. Large-output previews (E10) are the likely cause |
| E10 | The client cuts a large Bash result before either deslag boundary. `seq 1 12000` (60,894 bytes): `session.append` received a 2,320-byte `<persisted-output>` preview, and a PostToolUse hook received `stdout` cut to 30,000 characters plus `persistedOutputPath`. The model reads the preview in both cases | Live runs L and H (see [Live verification](#live-verification)) | live verified | Neither path can compress Bash output above the client's persistence threshold on 2.1.299, so this is no difference between hook and mod. The exact threshold was not measured |
| E11 | `deslag codec --protocol 1` returns one complete envelope, keeps number lexemes and UTF-8 byte counts exact, reports observe mode as `would_change` with emitted bytes equal to the original, and in safe mode changes text only when the owner file reads `mod` | `cargo test --locked -p deslag-cli codec` | offline verified | |
| E12 | Owner `mod` makes the hook and the proxy stand down without a change to either: `read_owner` maps any value other than `hook`/`proxy` to `none`, the hook compresses only under `hook`, and the proxy decides new results only under `proxy` while it still replays pinned decisions | Code reading of [`crates/deslag-proxy/src/session.rs`](../../crates/deslag-proxy/src/session.rs) and [`crates/deslag-cli/src/main.rs`](../../crates/deslag-cli/src/main.rs); existing test `no_routed_field_is_replaced_unless_the_hook_owns_compression` | offline verified | No end-to-end test with a live hook and mod in one session |
| E13 | Bridge p95 is at or below the hook's p95 from 2 KiB to 1 MiB (table below). At 2 KiB p50 (observe) and above the cap, the bridge is within about 0.1 ms of the hook. Every predeclared native budget passes | `cargo bench --locked -p deslag-cli --bench mod_bridge` | offline verified | Excludes the client's `$.process.run` dispatch; see Q3 |
| E14 | The model reads the rewritten row, and the transcript `content` holds only the rewritten form | Live run A: a test-only stub codec replaced a random 16-hex-digit Bash output with a sentinel; the stored `content` is the sentinel | live verified | Run A asked only for `done`; runs B to D show the model reading the sentinel |
| E15 | The transcript keeps the tool's structured record (`toolUseResult`) as made, so the original output stays on disk under a mod, in forks too. The hook replaced that record ([ADR 0006](../decisions/0006-session-identity-and-replay.md), run A) | Live runs A and D: the random value occurs once in each transcript, inside `toolUseResult.stdout` | live verified | A mod is not a way to keep originals off disk |
| E16 | Stored rewrites survive `--resume`, `--continue` and `--resume --fork-session` in a fresh client process with the mod disabled | Live runs B, C, D: each answered the sentinel verbatim; the fork got a new session id | live verified | Model switch, reload, upgrade and disable inside a running session are still pending |
| E17 | `$.process.run` cuts stdout at 4 MiB without an error: requests for 16 MiB and 64 MiB returned 4,194,304 characters with exit code 0. A child past `timeoutMs` rejects the call (`aborted: still running after 1000ms`, 1,020 ms). No abort-signal parameter; "CLI only" | Live run P (probe mod), declarations at `bf2323b` | live verified (limit, timeout), documented (surfaces, signal) | The silent cut makes the envelope check in E3 mandatory. Whether the timed-out child is killed was not checked |
| E18 | `turn.step` results carry `usage: { input_tokens, output_tokens, cache_read_input_tokens, cache_creation_input_tokens, model } \| null`; `null` when no response arrived | Declarations at `bf2323b`; the observer counts `null` as `unknown` (unit test) | documented, offline verified for the counting | Live usage capture pending |
| E19 | `$.telemetry` refuses user-installed and administrator-installed mods | Built-in telemetry README at `bf2323b` | documented | Deslag keeps its own bounded pipeline |
| E20 | The experimental mod with the real `deslag codec` in safe mode stores the minified row in a live session: pretty `{"id": 7, "tags": ["a", "b"]}` output was stored as `{"id":7,"tags":["a","b"]}`, and `toolUseResult.stdout` kept the pretty form | Live run E | live verified | One small payload |
| E21 | `$.process.run` dispatch from a mod costs about 1 ms: `/usr/bin/true` p50 1.16 ms, p95 2.19 ms; `deslag codec` on the 64 KiB payload (observe) p50 2.02 ms, p95 2.50 ms against 1.67 ms p95 for the same call whole-process outside the client | Live run P, 30 samples each, `performance.now()` in the mod | live verified | Small sample; measured inside the hook, before `next` |

### Benchmark: hook versus codec bridge

Apple M5 Max, macOS, rustc 1.96.0, release build of this branch, concurrency 1, whole process (spawn, pipes, exit). Payloads are pretty JSON records generated by [`crates/deslag-cli/benches/mod_bridge.rs`](https://github.com/mfittko/deslag/blob/be427f6/crates/deslag-cli/benches/mod_bridge.rs); the 64 KiB Bash payload is the generator behind the 78,633-byte baseline event in [`docs/benchmarks/baseline.md`](../benchmarks/baseline.md). The client's `$.process.run` dispatch is not included.

| Payload | Path | Samples | p50 | p95 | p99 | stdout bytes | Exit 0 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 2 KiB | hook `--minify-json` | 200 | 1.35 ms | 1.74 ms | 1.88 ms | 1,716 | 200/200 |
| 2 KiB | codec safe | 200 | 1.33 ms | 1.41 ms | 1.48 ms | 2,097 | 200/200 |
| 2 KiB | codec observe | 200 | 1.38 ms | 1.56 ms | 1.85 ms | 558 | 200/200 |
| 64 KiB | hook `--minify-json` | 200 | 1.71 ms | 1.80 ms | 1.89 ms | 50,030 | 200/200 |
| 64 KiB | codec safe | 200 | 1.61 ms | 1.74 ms | 1.79 ms | 50,416 | 200/200 |
| 64 KiB | codec observe | 200 | 1.53 ms | 1.67 ms | 1.80 ms | 562 | 200/200 |
| 8 MCP blocks of 8 KiB | hook `--minify-json` | 200 | 1.95 ms | 2.03 ms | 2.16 ms | 49,981 | 200/200 |
| 8 MCP blocks of 8 KiB | codec safe | 200 | 1.93 ms | 2.01 ms | 2.10 ms | 53,544 | 200/200 |
| 8 MCP blocks of 8 KiB | codec observe | 200 | 1.84 ms | 2.00 ms | 2.12 ms | 3,968 | 200/200 |
| 1 MiB | hook `--minify-json` | 50 | 7.93 ms | 8.16 ms | 8.34 ms | 807,458 | 50/50 |
| 1 MiB | codec safe | 50 | 6.02 ms | 6.21 ms | 6.49 ms | 807,851 | 50/50 |
| 1 MiB | codec observe | 50 | 4.83 ms | 4.97 ms | 5.10 ms | 569 | 50/50 |
| 17 MiB | hook `--minify-json` | 5 | 4.95 ms | 5.04 ms | 5.04 ms | 0 | 5/5 (passes through) |
| 17 MiB | codec safe | 5 | 5.05 ms | 5.16 ms | 5.16 ms | 0 | 0/5 (exit 65, original kept) |
| 17 MiB | codec observe | 5 | 4.93 ms | 4.96 ms | 4.96 ms | 0 | 0/5 (exit 65, original kept) |

Budget check: 64 KiB codec p95 1.74 ms against hook 1.80 ms and the 25 ms ceiling, pass. 1 MiB codec p95 6.21 ms against 100 ms, pass. Above the cap: exit 65, no stdout, p95 5.16 ms against 250 ms, pass. The multi-block row has no predeclared budget; it is reported only. Client dispatch overhead, live (E21): p95 2.50 ms through `$.process.run` against 1.67 ms whole-process, about 0.8 ms added, against the 10 ms budget, pass. Memory, allocations and parallel tool calls were not measured.

The 2 KiB safe envelope is larger than its input because the report travels with the replacement. A bridge for large payloads should drop the report from the transport or keep it as the only content of observe responses.

### Comparison

| Area | PostToolUse hook (shipped) | Mod `session.append` (experimental) | Proxy |
| --- | --- | --- | --- |
| Rewrite target | Structured `tool_response` (Bash `stdout` only, MCP text blocks) | Model-visible `content`; Bash `stdout` and `stderr` arrive joined | Request bytes on the wire |
| Bash with stderr | Compresses `stdout`, keeps `stderr` | Whole content must parse as one JSON document, so these rows pass through (about 10 % of Bash rows in E9). Gap: when `stdout` is empty and `stderr` alone is one JSON document, the mod cannot tell it from `stdout` and minifies it, which the hook's stdout-only rule never does. Closing the gap needs the structured record, which a `session.append` hook does not get | Same as mod |
| History | Client stores the replacement (live, ADR 0006) | Client stores the row; it survives resume, continue and fork with the mod disabled (live) | Replay manifests (offline) |
| Original on disk | Removed from the transcript (live) | Kept in `toolUseResult`, forks included (live) | Kept (client stores originals) |
| Composition | Last writer wins; deslag scans settings for conflicts | Middleware chain; stored-row comparison detects overrides (offline) | Separate process |
| Native bridge | Client spawns the binary per tool result | Mod spawns the binary per row via `$.process.run` | Long-running process |
| Usage observation | None | `turn.step` usage per step (documented) | Response usage (planned) |
| Surfaces | Settings hooks | CLI only for `$.process.run` | Any client that honors `ANTHROPIC_BASE_URL` |
| Install | Plugin plus verified binary | Same plugin, plus `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1` while Mods are early access | Launcher and credentials routing |

### Instrumentation placement

Option A (one installable mod, internal modules) is prototyped: the usage observer registers regardless of compression mode, so it works with compression off. Option B (a second installable mod) needs a versioned inter-mod contract. Two mods share no module state (E7); the only shared channels are events and `$.store`, a persistent key-value store that is neither a queue nor a safe concurrent counter. Nothing in this spike shows utility for a separately installed observer that Option A lacks.

### Live verification

Operator authorization (mfittko, 2026-10-14): USD 1.00 total, USD 0.30 per call, haiku only. Setup as in ADR 0006: a throwaway project directory, `--setting-sources project,local`, `--strict-mcp-config`, `--plugin-dir`, `--model haiku`, `--max-budget-usd 0.30`, `--output-format json`, `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1`, and a throwaway `DESLAG_STATE_DIR` whose owner file reads `mod`. No global settings were edited. Runs A and L used a test-only stub codec (outside the repository) that answers every slot with a sentinel and logs the size of what it received; the sentinel mechanism never ships. Run P used a test-only probe mod. Transcripts were read only for these throwaway sessions.

| Run | Purpose | Client | Model | Result | total_cost_usd |
| --- | --- | --- | --- | --- | --- |
| A | Mod (safe, stub) on `od -An -N8 -tx8 /dev/urandom \| tr -d ' \n'`; reply `done` | 2.1.299 | claude-haiku-5-5 | Stored `content` is the sentinel; `toolUseResult.stdout` holds the random value | 0.00167632 |
| B | `--resume A`, mod not loaded, Bash disallowed, quote the output | 2.1.299 | claude-haiku-5-5 | Answered the sentinel; same session id | 0.00561442 |
| C | `--continue`, same setup | 2.1.299 | claude-haiku-5-5 | Answered the sentinel | 0.00591479 |
| D | `--resume A --fork-session`, same setup | 2.1.299 | claude-haiku-5-5 | Answered the sentinel; new session id; the fork keeps the original in `toolUseResult` | 0.00613856 |
| L | Mod (safe, stub) on `seq 1 12000` (60,894 bytes) | 2.1.299 | claude-haiku-5-5 | The stub received 2,320 bytes: the `<persisted-output>` preview | 0.00118957 |
| H | Test-only PostToolUse capture hook on the same command | 2.1.299 | claude-haiku-5-5 | The hook received `stdout` cut to 30,000 characters, plus `persistedOutputPath` | 0.00067576 |
| P | Test-only probe mod: `process.run` output sizes, timeout, dispatch timing | 2.1.299 | claude-haiku-5-5 | 4 MiB stdout cap, silent; timeout rejects; about 1 ms dispatch (E17, E21) | 0.00117984 |
| E | Mod (safe) with the real `deslag codec` on pretty JSON | 2.1.299 | claude-haiku-5-5 | Stored row minified (E20) | 0.00122474 |

Total: USD 0.023614 of the USD 1.00 cap; the largest call was USD 0.0061 against the USD 0.30 per-call cap.

Live gate outcome:

- Q1 (preview ordering): resolved. Both boundaries see the client's cut (E10), so it does not separate hook from mod. Neither path compresses Bash output above the persistence threshold on 2.1.299.
- Q2 (model reads the rewrite; resume, continue, fork): pass (E14, E16).
- Q3 (`process.run` limit and overhead): 4 MiB silent cut, handled by the envelope check; about 0.8 ms added at p95, inside the 10 ms budget (E17, E21).

### Still pending

The remaining checks are tracked in the Pending checks list of the phase plan, [docs/phases/phase-mod-1.md](../phases/phase-mod-1.md#pending-checks).

## Recommendation

Verdict: conditional, with the compression gates now passed. Keep the hook as the Claude compression default (hybrid). Mod compression is cleared to proceed as an opt-in alternative (step 5), because its three live gates passed on 2.1.299. Choose the default at step 6 from a paired evaluation. Put instrumentation in the same mod as a separate internal module (Option A), once a live usage capture confirms E18. [ADR 0007](../decisions/0007-claude-mod-first-integration.md) records the decision.

Reasons:

- The engine mechanics work offline on 2.1.296 and 2.1.299 (E1 to E8) and live on 2.1.299 (E14, E16, E20). The bridge p95 is at or below the hook's from 2 KiB to 1 MiB and within about 0.1 ms elsewhere (E13); client dispatch adds about 1 ms (E21).
- Large Bash results are cut by the client before either boundary (E10), so the hook keeps no advantage there.
- The mod still trails the hook on three points: Bash rows with stderr pass through (E9), the stderr-only JSON gap (comparison table), and originals stay on disk (E15). `$.process.run` is CLI only (E17), and Mods need `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1` while they are early access.
- The usage observer can give deslag provider-reported usage without a proxy (E6, E18); the data itself still needs a live capture.

Staged plan. Step 1 is this spike: codec bridge, experimental mod, benchmark, live checks Q1 to Q3 and ADR 0007. Steps 2 to 6 graduated into the phase plan, [docs/phases/phase-mod-1.md](../phases/phase-mod-1.md). That file is the only copy of the staged plan and its step gates.

The issue impact and the session boundaries for owner changes are plan content, so they live only in the phase plan, [docs/phases/phase-mod-1.md](../phases/phase-mod-1.md).

Spike exit: graduated (operator decision, 2026-10-14) into [docs/phases/phase-mod-1.md](../phases/phase-mod-1.md), the plan for step 2 and the staged rollout.
