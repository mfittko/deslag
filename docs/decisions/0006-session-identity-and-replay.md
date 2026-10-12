# 0006: Session identity, ownership and deterministic replay

**Status:** accepted
**Date:** 2026-10-11
**Decided by:** mfittko

## Context

PLAN §3 invariants 5, 7 and 8 require that a submitted tool result keeps its representation, that recovery never silently rewrites history, and that one adapter owns compression per session. PLAN §8.3 asks M0 to choose one tested replay approach: deterministic replay with persisted per-result decisions, or explicit local representation storage.

The two adapters differ:

- Hook mode replaces the result inside the client. The client then owns the history.
- Proxy mode rewrites the request on the wire. The client keeps the original and resends it every turn, so the proxy must rebuild the same representation on every request.

This ADR records the decision, a live hook-mode resume experiment, and an offline proxy-mode replay experiment. The first real transform is out of scope.

Status update (#18): `deslag_proxy::session` implements the production manifests, retention (`RETENTION` and `prune`) and store-level owner enforcement. `crates/deslag-proxy/tests/session.rs` tests them. Proxy wiring (#72) and the command that writes `owner` are still pending.

## Decision

### Replay approach

Use deterministic replay with persisted per-result decisions. Do not store tool bodies.

- The first time an adapter sees a result, it decides `unchanged` or `encoded(transform id, transform version)` under the current policy. It records every new decision, including `unchanged`, atomically before it forwards the request that carries the result.
- On every later request, a known result gets its recorded decision, applied with the recorded transform version. The adapter never decides a known result again.
- An `unchanged` decision is recorded like any other. A result left unchanged because of load, size or parse failure stays unchanged for the life of the session.

Rationale: the decision record is small and holds no tool content. Explicit representation storage would hold tool output on disk and would need content-retention controls. Pure determinism without a record fails as soon as a resource bypass, a policy change or an upgrade changes the first decision. The offline experiment shows the record is enough to keep the provider-visible prefix byte-stable across resend, restart and upgrade.

### Pinned fields

Per session manifest:

| Field | Purpose |
| --- | --- |
| Scope digest | Session identity; the raw session id is not stored in exported data |
| Owner | Always `proxy`, because only the proxy writes a manifest; any other value is corrupt. The configured owner is read per request and is not pinned |
| Mode | `off`, `observe` or `safe`; a mode change applies to new sessions only. An `off` session transforms nothing and records no decisions. Its first request without tool results writes a manifest with mode `off` and no decisions. That manifest marks the session boundary, so the session keeps passing through after the configuration changes to `safe`. Configured `off` does not pass through a request that carries tool results under a scope with no manifest. It rejects that request with `MissingManifest`, as the other modes do |
| Model profile | Model in use when the manifest was created; each decision records its own model profile |
| Policy version | Policy in force when the manifest was created |

Per result:

| Field | Purpose |
| --- | --- |
| `tool_use_id` | Result identity within the scope |
| Source digest | Integrity check; changed source bytes under a known id are an explicit error |
| Decision | `unchanged`, or `encoded` with transform id and transform version |
| Policy version | Policy that made the decision |
| Model profile | Model of the request that first carried the result |

### Identity key

The per-result key is `(scope, tool_use_id)`. The source digest is a stored integrity check. A known id that arrives with a different digest is an explicit error and gets no new decision.

- Hook mode: scope is `session_id` from the hook stdin. Observed in run D.
- Proxy mode: scope is the `X-Claude-Code-Session-Id` request header. Claude Code 2.1.296 sets that header to its session id, and `metadata.user_id` carries a JSON string with `device_id`, `account_uuid`, `session_id` and an optional `parent_session_id`. This was read from the client binary and is untested.
- The key includes the scope. Run C showed that a fork keeps the parent's `tool_use_id` under a new session id.
- A request without a trusted scope passes through unchanged. The proxy never invents provider-visible fields.

### Ownership

This is the decided design. One configuration value, `owner = hook | proxy | none`, selects the owner. The launcher and doctor workflow will write it; no launcher or doctor writes `owner` yet. Both adapters read it, and each adapter transforms only when it is the owner. The configured owner file selects the deciding adapter and is read per request. Pinned decisions replay under any owner. The proxy decides new results only while the owner is `proxy`; under any other owner a new result is recorded `unchanged`. Only the proxy writes a manifest, and the manifest records owner `proxy`. A manifest whose owner and mode are not `proxy`/`safe` or `proxy`/`off` is corrupt state: it returns `MissingManifest` when the request carries tool results, and passes through otherwise. Tool-result content is never evidence of ownership. Unverifiable ownership means `none`. `crates/deslag-proxy/tests/session.rs` tests owner selection and the store-level owner checks.

### Behavior by event

| Event | Hook mode | Proxy mode |
| --- | --- | --- |
| Multi-turn resend | Client resends the replacement it stored (run B) | Replay recorded decisions; prefix byte-stable (offline) |
| Resume (`--resume`) | Client resends the stored replacement; session id unchanged (run B) | Same scope, same manifest; replay as for resend (design, untested live) |
| Fork (`--fork-session`) | Fork copies the stored replacement under a new session id (runs C and D) | New scope with no manifest, history already contains results: explicit error, start a fresh session. Unsupported (design, untested) |
| Process restart | No state needed for history; run B resumed in a new client process (run B) | Reload manifest from disk (offline) |
| Policy or software upgrade | History unaffected; new results use the new policy (design, untested) | Known results replay with the pinned transform version; new results use the new policy (offline) |
| Model switch | New results only (design, untested) | Known results keep their pinned decisions; each new decision records the new model profile (offline) |
| Mode changed from `off` to `safe` | Not applicable | An existing `off` session keeps passing through; new sessions use `safe` (offline) |
| Compression disabled | Stop replacing new results (design, untested) | Keep replaying pinned decisions; new results unchanged (design, untested) |
| Proxy removed mid-session | Not applicable | The client resends originals. Content stays correct, the cached prefix is lost. Start a new session (design, untested) |
| Manifest missing or corrupt | Not applicable | Explicit error when the request carries tool results, in every configured mode including `off`. A manifest that does not parse or has no `decisions` object counts as corrupt (offline) |
| Proxy added to a session with existing history | Not applicable | The scope has no manifest and the request carries tool results. Explicit `MissingManifest` error in every configured mode including `off`; the request is not passed through. This covers sessions started before deslag was installed and sessions started without the proxy. Start a new session (offline, as a lost manifest) |
| Source digest mismatch | Not applicable | Explicit error; the known id gets no new decision (offline) |
| Pinned transform id or version unknown to the binary | Not applicable | Explicit error (offline) |
| Malformed pinned decision | Not applicable | Explicit error. Only `unchanged: true` or `encoded` with a transform id and an unsigned integer version is valid (offline) |
| Unknown result outside the newest message | Not applicable | Explicit error (offline) |

Hook mode therefore needs no replay manifest for history. The client stores and resubmits the replacement. The decision record matters only for proxy mode.

Every binary keeps every transform version that a supported manifest references. Retiring a transform version needs a manifest migration, or it makes the affected sessions expire with an explicit error.

## Live hook-mode resume experiment

Setup: a throwaway project directory, `--setting-sources project,local`, `--strict-mcp-config`, `--plugin-dir` with a throwaway plugin, `--model haiku`, `--max-budget-usd 0.30` and `--output-format json`. Session persistence stayed on. No global settings were edited.

The replacing plugin used a `jq` PostToolUse hook on `Bash` that kept the other `tool_response` keys and set `stdout` to `REPLACED-BY-HOOK`, as in compatibility run 2. The empty plugin had no hooks. Run A ran `od -An -N4 -tx4 /dev/urandom`, so the model could not infer the original value from the command.

| Run | Purpose | Client | Model | Result | total_cost_usd |
| --- | --- | --- | --- | --- | --- |
| A0 | Replacement hook on `echo hello-deslag`; discarded because the model inferred the original from the command | 2.1.296 | claude-haiku-5-5 | Model saw `REPLACED-BY-HOOK` | 0.00187567 |
| A | Replacement hook on a random 8-hex-digit value; model replied `done` | 2.1.296 | claude-haiku-5-5 | Hook stdin `tool_response.stdout` held the random value; model saw the replacement | 0.00119555 |
| B | `--resume <A>`, empty plugin, Bash disallowed, asked for the verbatim stdout | 2.1.296 | claude-haiku-5-5 | Answered `REPLACED-BY-HOOK`; `session_id` unchanged | 0.00500765 |
| C | `--resume <A> --fork-session`, empty plugin, same question | 2.1.296 | claude-haiku-5-5 | Answered `REPLACED-BY-HOOK`; new `session_id` | 0.00532302 |
| D | `--resume <A> --fork-session` with the replacing hook, one new Bash call | 2.1.296 | claude-haiku-5-5 | Hook stdin `session_id` was the new fork id | 0.00582497 |

Total: USD 0.01922686. The operator (mfittko) allotted USD 2.50 of the shared M0 budget of USD 5 to DF-004. Cumulative M0 live spend against the USD 5 cap is USD 0.0038538 (DF-002) + USD 0.01922686 (DF-004) = USD 0.02308066.

Transcript inspection (read only, `~/.claude/projects/<project>/<session>.jsonl`):

- The session from run A stores the result as `{"type":"tool_result","content":"REPLACED-BY-HOOK"}`, and its `toolUseResult` holds the replaced object with `stdout: "REPLACED-BY-HOOK"`. The random value occurs zero times in the file.
- The fork transcript from run C contains the same `tool_use_id` as the parent, the replacement, and zero occurrences of the random value. Every entry carries the new session id.
- The original output exists only in the hook stdin. The client keeps no copy that a resume could restore.

`--continue` was not run. It resumes the newest session by the same path as `--resume`, so it is expected to behave like run B. It stays pending.

## Offline proxy-mode experiment

`crates/deslag-proxy/tests/replay_experiment.rs` holds an experiment-only replay layer over Messages request JSON. Each request goes through the real deslag proxy to a local mock upstream, and the test asserts the bytes the upstream saw. A minify-based toy transform with versions 1 and 2 stands in for real transforms. A temp directory holds the manifests. The layer is experiment-only; `deslag_proxy::session` holds the production manifests and retention.

Scenarios:

- Multi-turn resend and restart: three turns and an identical resend of the third, then a process restart and a fourth turn. Each upstream body starts with every message byte of the previous body. An identical resend is byte-identical.
- Policy upgrade: a session built under policy v1 is resent under policy v2 with transform v2. Results decided under v1 keep their v1 bytes. This includes `toolu_2`, which v1 left unchanged and which a fresh v2 decision encodes; the test asserts both premises. A new result uses v2.
- First decision unchanged: a result left unchanged under simulated load stays unchanged after the load drops.
- Manifest loss: a corrupt or deleted manifest with tool results in the request is an explicit error in every configured mode, including `off`. The `off` case uses a scope that never had a manifest, which is the same input as a proxy added to a session with existing history. Corrupt covers bytes that do not parse and a manifest with no `decisions` object. A pinned transform id or version that the binary does not know is an explicit error too. So is a malformed pinned decision: one that does not carry exactly one of `unchanged: true` or `encoded` with a transform id and an unsigned integer version. Examples are version `1.0`, `"1"` or `-1`, and a decision with both `unchanged` and `encoded`. A malformed decision never replays as unchanged. The replay layer rejects the request before the proxy is called.
- Digest mismatch and undecided history: changed source bytes under a known `tool_use_id`, or an unknown result outside the newest message, is an explicit error. The replay layer rejects the request before the proxy is called.
- Model switch: a result decided under model `m` keeps its decision after the request model changes to `m2`. A new result records `m2` as its model profile. The session manifest keeps `m`.
- Off session: an `off` session writes a manifest with mode `off` and no decisions. After the configuration changes to `safe`, that session still passes through byte-identical, and a new session transforms.
- No scope: the body passes through byte-identical and no manifest is written.

Recorded output of `cargo test --locked -p deslag-proxy --test replay_experiment -- --nocapture --test-threads=1`:

```text
running 9 tests
test digest_mismatch_and_undecided_history_are_explicit_errors ... replay: Err(DigestMismatch("toolu_1")); Err(UndecidedHistory("toolu_9"))
ok
test first_unchanged_decision_stays_unchanged ... replay: toolu_1 left unchanged under load stayed unchanged after load dropped
ok
test manifest_loss_and_retired_transform_are_explicit_errors ... replay: corrupt and missing manifest -> Err(MissingManifest)
ok
test model_switch_records_model_per_decision ... replay: model switch m->m2 kept toolu_1@m; toolu_3 recorded m2
ok
test multi_turn_resend_and_restart_keep_prefix_byte_stable ... replay: 4 turns + restart, upstream body bytes [87, 364, 586, 838], toolu_1 73 -> 57 bytes
ok
test no_scope_passes_bytes_through ... ok
test off_session_stays_off_after_mode_change ... replay: off session passed through after mode changed to safe
ok
test pinned_decision_forms_are_validated ... replay: 12 pinned decision forms, 3 replayed, 9 explicit errors
ok
test policy_upgrade_replays_pinned_decisions ... replay: upgrade v1->v2 kept toolu_1@v1 and toolu_2 unchanged; toolu_3 used v2
ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

A mutation that re-decides known results under the current policy fails the upgrade and first-unchanged scenarios on the prefix assertion.

## Unsupported cases

- Proxy-mode fork. The fork arrives under a new session id with history the new scope never decided. The proxy returns an explicit error, and the user starts a fresh session. Linking a fork to its parent through `parent_session_id` is untested.
- Proxy-mode requests without a trusted session id. They pass through unchanged.
- Proxy-mode resume and restart with a live client. Covered offline only.
- Subagent requests in proxy mode. Untested.
- Concurrent requests for one scope. The experiment layer does an unlocked read-modify-write; production needs a per-scope lock.
- Tool results with block-array, image or other non-string content. The experiment passes them through undecided.
- Manifest loss, corruption or expiry for a session with established history. Explicit error; there is no recovery path.
- A `safe` session that loses its manifest before it records any decision, and then sends a request without tool results under configured `off`, looks like a fresh session. It gets an `off` marker. No decisions are lost.
- Adding the proxy to a session with existing history, including sessions started before deslag was installed and sessions started without the proxy. A request whose scope has no manifest and that carries tool results gets an explicit `MissingManifest` error in every configured mode, including `off`. The proxy does not pass these requests through. The user starts a new session.
- Removing the proxy from a session with encoded history. The client resends originals and the cached prefix is lost.
- Changing `mode` inside a running session. It applies to new sessions only. The configured owner is read per request: pinned decisions replay under any owner, and new results are decided only while the owner is `proxy`.
- Owner enforcement: the command that writes `owner` is pending. Store-level owner checks are tested in `crates/deslag-proxy/tests/session.rs`; proxy wiring (#72) is pending.
- Proxy-mode compression-disabled replay. Untested. Model switch and the `off` session marker are covered offline only.
- `--continue`, Linux and Windows clients, and client versions other than 2.1.296. Untested.

## Consequences

- Hook mode is the simpler owner for Claude Code: the client keeps the replacement across resume and fork, so history needs no deslag state.
- Proxy mode needs an atomic manifest write before it forwards any request that carries a new result, a retained transform version for every pinned decision, and an explicit error path. `deslag_proxy::session` implements production manifests, retention and atomic commits; wiring them into the proxy (#72) is the next step for proxy replay.
- Proxy-mode generic mutation stays limited to sessions with a trusted session id that are neither forked nor recovered without a manifest.
- A canonicalizing JSON round trip is acceptable only in the experiment. Production replay must splice changed blocks into the original request bytes, so unknown fields and key order stay intact.
