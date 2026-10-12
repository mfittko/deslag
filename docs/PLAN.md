# deslag - Implementation Plan

**Status:** Implementation handoff; no implementation is claimed by this document.  
**Planning date:** 2026-10-10  
**Initial release:** v0.1, Claude Code plugin plus a minimal Anthropic-compatible proxy  
**Implementation decision:** Rust, unless a documented compatibility or benchmark result justifies changing it.

> A small, native, deterministic context-compression layer for AI agents. Reduce the representation overhead of tool results without changing executed tool calls, silently dropping evidence, or putting telemetry delivery on the request path.

## 0. Instructions for the implementing agent

Treat this document as the product brief, architectural baseline, and initial backlog. It describes the system to build, not functionality that already exists.

1. Inspect the target repository before creating or changing its structure. Preserve existing work and established conventions.
2. Start with milestone M0. Verify the actual Claude Code hook contract, supported versions, provider behavior, and upstream reference licenses before relying on them.
3. Implement small, independently testable vertical slices. Each change must include appropriate fixtures, tests, documentation, and a clear report of what was actually verified.
4. Keep the compression engine independent from Claude hooks, HTTP, credential handling, persistent state, and telemetry exporters.
5. Prioritize deterministic structured-data compression and a usable Claude Code plugin. Do not expand into a general LLM gateway or agent framework.
6. Use the non-negotiable invariants in section 3 as review criteria. Do not trade them away to improve a benchmark.
7. Record material deviations in a short architecture decision record (ADR). Do not silently broaden scope, weaken acceptance criteria, or describe untested behavior as supported.
8. Maintain a progress record containing completed backlog IDs, commands run, test results, benchmark artifacts, unresolved compatibility limitations, and the next concrete task.

When credentials, an installed client, or a paid evaluation budget are unavailable, complete offline work and clearly mark the remaining live verification as pending. Never fabricate compatibility, latency, token savings, or task-quality results.

### First executable slice

Build a no-op Claude hook adapter and a transparent streaming Anthropic proxy against a local mock upstream. Then add one JSON-minification transformation through the shared core and demonstrate it in both adapters. Do not begin with a dashboard, retrieval store, or generalized plugin framework.

## 1. Product intent and user requirements

The project is named **deslag**. It is inspired by [Headroom][S1], but should be a focused implementation rather than a feature-for-feature clone.

| User requirement | Planned interpretation |
| --- | --- |
| Proxy in front of any agent harness | Harness-independent architecture with explicit, incrementally tested protocol adapters. Clients need a supported interception boundary or configurable provider endpoint. |
| Focus on the biggest levers | Compress model-visible JSON and repetitive tool-result text. Target repeated object keys, verbose formatting, and exact repetition before subjective summarization. |
| Maximum performance; Rust or Go | Choose Rust for the core and executable. Measure the complete path, including parsing, token estimation, hook startup, state handling, and streaming. |
| Ship as an MVP Claude plugin | Deliver a Claude **Code** plugin backed by the native executable. Prefer native result replacement where verified; retain explicit proxy/launcher integration. |
| Observability at some point | Design typed reports and local measurements into v0.1. Add optional asynchronous OpenTelemetry/OTLP export in v0.2. |
| Follow the user's LLM proxy architecture | Reuse the non-blocking instrumentation principle from [sofatutor/llm-proxy][S3], not its entire management, database, or dispatcher stack. |

The primary outcome is **reliably completing the same agent work with less model context and no offsetting increase in total usage or latency**. A large compression ratio on synthetic JSON is not sufficient.

Here, compression means changing the model-visible representation. HTTP gzip/DEFLATE, transport minification, and archive compression are not the product's token-saving mechanism.

## 2. Decisions and boundaries

### 2.1 Architectural decisions

| Decision | Rationale |
| --- | --- |
| One native binary and a reusable Rust core | Straightforward distribution and a shared implementation for hooks and proxies. |
| Native Claude result hook first; generic proxy alongside it | The hook can transform a result before its first model submission without intercepting credentials. The proxy supports other harnesses and existing gateway deployments. |
| Protocol preservation, not protocol translation | Avoid reconstructing every provider's API through a universal message schema. |
| Tool results, not tool execution | Executable arguments, commands, patches, and tool definitions are not compression targets in v0.1. |
| Deterministic transformations | Results must be reproducible across repeated submissions and session resumes. |
| Observe mode initially | Establish what would change and its estimated benefit before enabling mutation. |
| Metadata-only telemetry by default | Avoid copying sensitive payloads into observability infrastructure. |
| Optional OTLP export, preferably through a Collector | Keep backend integrations outside deslag and avoid mandatory infrastructure. |
| No local model or compression API | Compression must not add inference cost, remote dependencies, or model latency. |

Headroom already has Rust core and proxy crates; a language rewrite alone is not a differentiator. Its workspace and transform modules are useful implementation references, but its broader feature set is deliberately outside this MVP. [S2]

### 2.2 v0.1 scope

- Shared classification, transformation, safety, and reporting core.
- JSON minification within eligible tool-result text.
- Constrained repeated-record encoding for homogeneous JSON arrays.
- Constrained exact consecutive-line repetition encoding.
- Claude Code plugin with version/shape checks, diagnostics, and explicit enable/disable behavior.
- Minimal Anthropic Messages proxy with transparent streaming and tested history handling.
- Local compression statistics, including skipped and failed transformations.
- Offline fixtures, property tests, replay tests, benchmarks, and an initial paired task-evaluation harness.
- macOS arm64 and Linux x86_64 release artifacts as initial platform targets. Add other targets only when their builds and smoke tests exist.

Structural and repetition encodings are deliverables, but remain opt-in until their quality gates pass. If a transform fails evaluation, ship the working product with that transform disabled rather than weakening the gate.

### 2.3 Deferred scope

OpenAI Chat Completions and Responses adapters, OTLP export, richer operational telemetry, and a container distribution belong to v0.2. Tool-specific lossy policies and optional original-content retrieval belong to v0.3 or later.

Explicitly exclude from the MVP:

- Prose summarization, conversation compaction, semantic reranking, arbitrary truncation, and search-result sampling.
- AST/code rewriting, generated patch compression, and tool-schema pruning.
- Prompt optimization, automatic reasoning-effort changes, output shaping, and model routing.
- Cross-agent memory, self-improvement loops, RAG, embeddings, and local ML models.
- Provider credential issuance, project billing, token management, response caching, or an admin UI.
- Required Redis, Kafka, SQL databases, external services, or a distributed event bus.
- Automatic support claims for every client, transport, authentication mode, or provider.

## 3. Non-negotiable invariants

1. **Do not change what tools execute.** Preserve tool-call names, IDs, arguments, commands, patches, permissions, and executable payloads.
2. **Do not silently discard evidence.** Do not drop rows, distinct lines, errors, IDs, timestamps, or values under the label of safe compression.
3. **Preserve provider protocol semantics.** Keep roles, message ordering, tool-call/result pairing, error flags, content block types, and unknown fields intact.
4. **Do not mutate protected content.** System/developer/user instructions, assistant messages, thinking/signature blocks, encrypted state, media, and unknown content types are outside the default target set.
5. **Choose a representation once.** Previously submitted tool results must not change because of load, time, a new policy, a model switch, or a software upgrade.
6. **Unsupported new content passes through.** Parser uncertainty, insufficient gain, resource limits, or unsupported schemas must not lead to partial output or truncation.
7. **Fail-open is history-aware.** Falling back for a new result is different from forgetting the representation of an already submitted result. Never silently rewrite established history.
8. **One compression owner per session.** Hook and proxy paths must not both transform the same session.
9. **Keep telemetry non-blocking.** Exporter outages and full telemetry queues must not stall requests, tools, or streamed responses.
10. **Bound resources.** Define limits on bytes, depth, candidate count, concurrency, IPC messages, telemetry queues, and retained session metadata.
11. **Keep content untrusted.** Compression does not sanitize prompt injection or make tool output authoritative. Preserve content boundaries and never execute instructions found in a payload.
12. **Separate evidence from estimates.** Bytes, estimated tokens, provider-reported usage, and inferred economic savings are different measurements.
13. **No silent credential rerouting.** Forward credentials only to the explicitly configured upstream. Do not alter client authentication or scrape credential stores.
14. **No hidden inference.** Never call an LLM to classify, compress, summarize, or repair a result in the default pipeline.

## 4. Verified reference constraints and compatibility work

These references inform the plan; they do not replace testing against the client and SDK versions actually shipped.

### 4.1 Claude Code

The official hook reference checked on 2026-10-10 documents `PostToolUse` replacement through `updatedToolOutput`, including built-in tools. Replacements must match the tool's output shape; invalid built-in replacements are ignored. It also states that native tool telemetry may capture the original result before the hook runs. [S5]

Consequences for deslag:

- Verify the minimum working client version and test each supported result shape.
- Preserve surrounding structures and change only approved text fields.
- Finish compression before returning the replacement; do not run this transformation as a background hook.
- Do not market the plugin as a redaction layer for the client's own telemetry.
- Keep hook stdout restricted to the documented protocol; write diagnostics elsewhere.
- Preserve error output and non-text blocks. Unsupported failure-event behavior is observation-only until verified.

Plugin-provided settings have a restricted supported surface; do not assume a plugin can set arbitrary runtime environment or routing keys. Use the documented manifest/hook format and an explicit launcher or setup operation for proxy routing. [S6]

Claude Code documents custom gateway connection through `ANTHROPIC_BASE_URL`. Validate the current configuration, required endpoints, headers, and authentication behavior against the actual client before advertising compatibility. [S7]

### 4.2 Provider caching and state

Anthropic documents exact-prefix requirements for prompt-cache reuse. Design for stable content, ordering, and provider-specific state, and test thinking/signature behavior against the selected model and API options rather than assuming every mode has identical rules. [S8]

### 4.3 Async instrumentation reference

The reviewed `llm-proxy` implementation uses a bounded, non-blocking publication path and tracks drops. Its subscriber dispatch loop retries serially; deslag should not reproduce that coupling between slow consumers. The instrumentation docs also describe body capture, which deslag must not adopt as its default. [S3], [S4]

OpenTelemetry span lifecycle callbacks execute synchronously. The batching processor provides bounded buffering, with spans dropped at queue capacity. Use non-blocking callbacks and bounded export; an asynchronous exporter does not make expensive attribute construction or payload copying free. [S9]

## 5. System architecture

```text
Claude Code
  -> plugin PostToolUse adapter
  -> eligible result-field extraction
  -> deslag-core
  -> shape-preserving replacement
  -> Claude Code conversation

Other supported harnesses / explicit Claude proxy mode
  -> deslag HTTP proxy
  -> protocol-specific eligibility and history checks
  -> deslag-core
  -> patch selected content; preserve everything else
  -> configured upstream provider or existing gateway
  <- streaming response relayed unchanged

Both paths
  -> small compression reports / bounded counters
  -> local statistics
  -> optional asynchronous OpenTelemetry export (v0.2)
  -> optional Collector -> chosen backend
```

### 5.1 Responsibilities

| Component | Owns | Must not own |
| --- | --- | --- |
| Core | Classification, eligible transforms, bounded processing, gain decisions, reports | HTTP, credentials, filesystem state, hook protocol, exporters |
| Claude adapter | Hook schema, field selection, session/subagent scope, output envelope | Provider billing claims, arbitrary message rewriting |
| Proxy adapter | HTTP semantics, provider-specific extraction, history consistency, streaming | Tool execution, provider-to-provider translation |
| Session layer | Policy/version pinning, compression ownership, recorded decisions, recovery | Analytics, cross-session content deduplication |
| Telemetry layer | Metrics, reports, bounded IPC/export, sampling, drop accounting | Required storage for reconstructing conversation history |
| CLI/plugin packaging | Installation, diagnostics, launch integration, safe configuration | Hidden global settings changes or automatic credential migration |

### 5.2 Proposed repository layout

Adapt names to an existing repository where necessary; do not create empty abstraction layers solely to match this tree.

```text
Cargo.toml
Cargo.lock
PLAN.md
README.md
crates/
  deslag-core/
    src/{classify,transforms,policy,report}.rs
  deslag-runtime/
    src/{hook,proxy,session,local}.rs
  deslag-telemetry/
    src/{metrics,report_sink,otlp}.rs
  deslag-cli/
    src/main.rs
plugins/
  claude/
    .claude-plugin/plugin.json
    hooks/hooks.json
    commands/
    scripts/
tests/
  fixtures/
  protocol/
  replay/
  evaluations/
benchmarks/
docs/
  architecture.md
  compatibility.md
  compression-formats.md
  observability.md
  benchmark-methodology.md
  decisions/
```

Choose maintained Rust libraries for HTTP, async execution, JSON, CLI handling, testing, and OpenTelemetry after checking current compatibility. Pin dependencies and the toolchain. Avoid mixing Rust and Go or adding an FFI layer without measured need. No Python or Node runtime should be required by the installed product; development-only tooling is a separate decision.

## 6. Core API and processing pipeline

### 6.1 Core contract

The following is a design sketch, not final compilable API:

```rust
fn compress<'a>(
    input: &'a str,
    context: &CompressionContext,
) -> CompressionResult<'a>;

struct CompressionResult<'a> {
    output: Cow<'a, str>,
    report: CompressionReport,
}
```

The context supplies a pinned policy, supported output category, resource limits, selected transform/format version, and an optional local token-counting capability. It must not provide the core with network clients or mutable global session state.

A no-op should borrow its input where practical. An output is committed by the adapter only after all safety, gain, and consistency checks succeed. Errors must not expose partial rewrites.

Recommended report fields:

```text
schema_version
transform_id and transform_version
format_version
policy_version
mode: off | observe | safe
outcome: changed | would_change | skipped | failed
skip_or_failure_reason: bounded enum
original_bytes
candidate_bytes
emitted_bytes
original_tokens: optional count + method + tokenizer/version
candidate_tokens: optional count + method + tokenizer/version
emitted_tokens: optional count + method + tokenizer/version
compression_duration
validation_outcome
```

Count absence as unknown, not zero. Keep identifiers and timestamps that belong to the integration outside the pure transformation result. In observe mode, candidate savings are hypothetical; emitted content remains original.

DF-010 finalized this contract in `deslag-core` (`compress`, `CompressionContext`, `CompressionReport`). `CompressionReport::to_json` writes the fields above as snake_case keys; `compression_duration` is whole nanoseconds and every absent value is `null`. The core owns the §11 limits `min_candidate_bytes`, `max_candidate_bytes` and `max_json_depth` (`Limits::default()` uses the §11 values) and the `json_minify`, `exact_line_runs` and `line_templates` switches. `Limits::max_table_rows` (default 20000) and the opt-in `json_table` switch now exist with table-v1 (DF-012). DF-014 provides the local `EstimateCounter` (method `estimate`, tokenizer `deslag-estimate-v1`); token gain thresholds arrive with #15. The processing budget and parallel job limit belong to adapter-side bounded execution (DF-015). The supported output category and the transform/format version selection in the context remain open after table-v1.

### 6.2 Pipeline

1. Check integration eligibility, session ownership, and history consistency before asking the core to transform anything.
2. Apply cheap content-type and size gates. Inspect only approved tool-result fields.
3. Identify a supported format without recursively interpreting arbitrary text as new instructions or schemas.
4. Validate JSON/text shape and resource limits; preserve raw lexical values where required.
5. Select a deterministic transform for this content and pinned policy.
6. Produce a bounded candidate; do not construct unbounded alternative representations.
7. Validate the candidate and calculate its net gain, including all wrappers and format metadata.
8. Return original content if there is insufficient gain or any uncertainty about correctness.
9. Record the final decision at the adapter/session boundary before it can become established history.
10. Emit only a small report to the telemetry sink. Failure to deliver the report must not change the compression decision.

Off mode performs no candidate transformation. Observe mode calculates proposed output but emits the original. Safe mode enables only approved transforms for supported content and tested model profiles.

### 6.3 Token accounting

Measure byte counts exactly. Count tokens only with an explicitly identified local tokenizer, or label the result as an estimate. A tokenizer borrowed from another model family is not an exact provider count.

Do not invoke a provider token-counting API on the production hot path. Provider count endpoints may be used in authorized offline validation. Scope tokenization to candidate content and cache reusable tokenizer initialization; do not tokenize the full conversation on every request.

Require a meaningful gain margin before applying a candidate. When an exact tokenizer is unavailable, use a conservative calibrated estimate and report that uncertainty. Do not guarantee token reduction merely because the byte count fell. Unknown model profiles should initially bypass structural encodings until evaluated.

## 7. Initial compression strategies

### 7.1 JSON minification: first working transformation

Target valid JSON contained in model-visible tool-result text. Remove only insignificant whitespace outside strings.

Do not confuse this with minifying the outer API request: transport formatting is not the model-visible content being optimized.

Implementation rules:

- Use a validating, lexical-preserving approach rather than a generic parse/serialize round trip that may normalize data.
- Preserve string contents and escapes, numeric lexemes, object member order, and array order.
- Skip ambiguous or unsupported inputs, including duplicate keys for the initial implementation.
- Preserve negative zero, exponent notation, large integers, escaped Unicode, and whitespace inside strings.
- Do not interpret strings containing JSON as another recursive transformation target. One exception applies: behind the opt-in `json_table` switch, table-v2 may encode a string cell whose text is minified JSON as a sub-table. That encoding is lossless, restores the original string lexeme byte for byte, is validated by decoding, and has a bounded nesting depth (see table-v2 Sub-tables in [compression-formats.md](compression-formats.md)).
- Do not apply when a tool category or explicit policy requires exact original bytes.

Acceptance tests: semantic round-trip checks plus lexical-value checks; invalid/truncated JSON passes through unchanged; byte-sensitive categories are bypassed; no token-saving claim is made solely from removed whitespace.

### 7.2 Repeated-record encoding: main structural lever

For sufficiently large homogeneous arrays of objects, represent the field names once and retain every row and value.

Illustrative format, subject to freezing in `docs/compression-formats.md`. DF-012 froze table-v1 there, and table-v2 extends it to arrays inside a wrapper object. That specification supersedes the illustrative format and the initial eligibility list below. For example, it allows missing fields and key orders that are subsequences of the column order.

```json
{
  "_deslag": {"format": "table-v1"},
  "columns": ["id", "status", "owner"],
  "rows": [
    [101, "open", "alice"],
    [102, "closed", "bob"]
  ]
}
```

This small example demonstrates shape, not savings. Include the marker, field names, delimiters, escaping, and any required explanation in gain calculations.

Initial eligibility:

- Top-level array of objects with identical keys in identical order.
- Supported scalar values only: strings, numbers, booleans, and null.
- No duplicate keys, missing fields, mixed schemas, nested arrays/objects, or irregular records.
- Sufficient size and net gain under the selected counting method.
- A model/profile evaluation permitting this format.

Preserve all rows, order, numeric lexemes, string values, nulls, and explicit field relationships. Null is not a substitute for a missing field. Preserve raw scalar tokens when encoding rows.

Provide a local decoder for testing. Its contract reconstructs the original ordered JSON data and scalar lexemes, not removed formatting whitespace. Test collisions and payloads that already resemble the wrapper. The wrapper is data, not authorization to alter unrelated content.

Do not inject an extra system instruction on every call to explain the encoding. Prefer a self-describing representation. If a fixed format explanation proves necessary, evaluate its cost and define a stable, explicit integration; do not silently mutate system prompts.

**Reconstructability is not model-quality equivalence.** Keep this transform opt-in until task-level evaluation passes.

### 7.3 Exact consecutive-line repetition

Compress runs of byte-identical consecutive lines in supported plain-text tool outputs. Keep exact line content, line endings, run position, repetition count, and the final-newline distinction reconstructable.

Use an unambiguous versioned representation with escaping. Define it before enabling it. A structured run encoding is acceptable, but apply it only when wrapper overhead is outweighed by actual gain.

In the exact-repetition codec (`lines-v1`), do not merge similar lines with different timestamps, IDs, paths, counts, or error messages. Do not reorder nonconsecutive repetitions. Do not suppress error lines or successful test output using relevance heuristics.

A decoder must reconstruct the original text bytes exactly. Mixed LF/CRLF, empty lines, partial final lines, and strings resembling encoding markers need tests.

DF-013 implements this as `lines-v1`, specified in [compression-formats.md](compression-formats.md).

The line-template codec implements templates for consecutive lines that differ only in variable tokens as `lines-v2`, specified in [compression-formats.md](compression-formats.md). It is a separate opt-in transform and the only exception to the merge rule above. It merges lines that differ only in variable-class tokens. A variable-class token contains an ASCII digit, `/` or `\`, which covers most timestamps, IDs, counts and paths. A constant token that differs, such as a word in an error message, still ends a template, so those lines are never merged.

### 7.4 Transform composition and exactness

Make each approved transform idempotent for its supported representation, and test the complete pipeline for repeated application. Do not recursively compress strings inside an encoded wrapper. The only exception is the table-v2 sub-table encoding from §7.1: lossless, decode-validated, depth-bounded and enabled only by `json_table`.

Distinguish three guarantees in documentation:

- Exact byte passthrough for untouched content.
- Preservation of JSON data and raw scalar values, but not formatting, for JSON transformations.
- Exact reconstructed text for the repetition codec.

Never label all three as universally byte-lossless. A task asking for literal original output may depend on formatting; such tasks require an explicit bypass or a later retrieval-backed design. Do not assume an intent classifier can reliably detect every byte-sensitive task.

## 8. Conversation stability, policy pinning, and recovery

### 8.1 Representation identity

Establish a stable identity for a result using an adapter-provided scope, such as session/subagent identity, tool-result identity or position, and a scoped source digest. Do not rely on a tool-call ID alone being globally unique.

Pin at least:

```text
session/branch scope
compression owner: hook | proxy | none
policy version and mode
transform implementation / format versions
model profile used when choosing the representation
per-result source identity
per-result decision: unchanged | encoded with version X
```

Source digests and session IDs stay out of metric labels and exported payloads by default. They are correctness metadata, not analytics dimensions.

### 8.2 Every first decision matters

Record unchanged decisions as well as transformed ones. A result initially left unchanged because of overload must not suddenly be compressed when the same history is resent under lighter load.

Changing from observe to safe applies to a new session, branch, or explicitly supported fresh boundary. It must not reprocess the established prefix. A model switch changes eligibility for future results, not historical representations.

Disabling compression means stop transforming new results. Existing compressed history may still need its pinned representation. Document when leaving proxy mode requires a new session instead of pretending a global off switch can rewrite provider state safely.

### 8.3 Hook and proxy differences

For hook mode, verify that the client stores and resubmits the replacement rather than regenerating the original result after a resume or fork. Do not assume transcript files are a synchronous or authoritative interception mechanism.

For proxy mode, expect the harness to resend its original content. The proxy must reproduce the representation the provider previously saw. The M0 spike must choose and document one tested approach:

- Deterministic replay under pinned, retained implementation versions, with persisted per-result decisions; or
- Explicit local representation storage with content-retention controls.

Prefer the first to avoid storing tool bodies. Pure determinism is insufficient if resource-dependent bypasses, policy changes, or software upgrades can change the first decision.

If the harness supplies no trustworthy session identity, do not invent provider-visible fields or merge unrelated sessions. Use a proven stable, scoped replay design or operate in passthrough/observe mode with the limitation documented. Generic mutation support depends on solving this boundary.

### 8.4 Persistence is not telemetry

Use small local session manifests if required; a server database is not an MVP dependency. Correctness state may require an atomic write before a changed request is forwarded. Measure that cost rather than moving a required commit onto a lossy telemetry queue.

Never evict replay-critical metadata for active/resumable sessions silently. Define retention, supported resume lifetime, atomic writes, version migration, and explicit expiration behavior. If reliable state cannot be established for a new session, keep that session unmodified.

After recovery failure, do not silently send a different old prefix. Return an actionable integration error or require a fresh session according to the documented adapter behavior. Never silently bypass the configured proxy to send credentials directly elsewhere.

### 8.5 One compression owner

Use an explicit session configuration or trusted local handshake to select hook or proxy ownership. A user-controlled string inside a tool result is not proof of ownership.

A proxy downstream of the active hook adapter should forward that session's content without further compression. Unverifiable ownership defaults to the conservative configuration established by the launcher/doctor workflow.

## 9. Claude Code plugin and local executable

### 9.1 Plugin responsibilities

Provide the native hook integration, explicit onboarding, status, diagnostics, and disable instructions. Do not depend on the model remembering to invoke a compression tool.

The hook adapter must:

- Validate the event and supported output schema.
- Match only approved tool categories and fields; initially prefer Bash stdout and supported MCP text results.
- Preserve stderr, error/status fields, media, structured non-text results, and unknown fields unless separately approved.
- Produce one complete valid replacement envelope, or no replacement.
- Avoid modifying permission decisions, classifier behavior, tool arguments, or side effects.
- Handle parallel tools and subagents without cross-session contamination.
- Return promptly on missing configuration, unavailable optional local services, or unsupported content.
- Keep diagnostics and telemetry separate from stdout.

Support for MCP `structuredContent`, schemas, resources, images, or binary attachments must be explicit; do not treat all MCP fields as freely replaceable strings.

Test hook ordering and conflicts with other result-replacement hooks. When reliable composition cannot be established, report the conflict rather than overriding another plugin unpredictably.

### 9.2 Distribution and lifecycle

Use official plugin manifests and installation mechanisms verified during M0. Package or resolve a version-matched native executable for the supported OS/architecture.

No compiler, Python environment, or Node installation should be required at runtime. Validate release checksums and provenance. A lifecycle hook must not download and execute an arbitrary latest binary, silently edit global settings, or replace a user-managed executable.

Keep plugin cache locations separate from writable user state. Use private permissions for state and IPC endpoints. Pin compatible versions for resumed sessions; do not perform an unannounced in-session upgrade.

### 9.3 Proposed CLI surface

These are proposed commands, not existing functionality. Finalize and test their syntax during implementation.

```text
deslag compress       Offline transform/observe mode; stdin or explicit file input.
deslag hook claude    Hook JSON on stdin; protocol JSON only on stdout.
deslag proxy          Run the configured HTTP proxy.
deslag run -- <cmd>   Launch a child with explicit, scoped proxy configuration.
deslag doctor         Check versions, routing, ownership, schemas, and state access.
deslag status         Show mode, versions, supported capabilities, and active services.
deslag stats          Read locally available aggregate measurements.
deslag local          Optional local report aggregation service; start/stop/status.
```

Doctor output must redact credentials and explicitly distinguish hook mode from proxy mode. Plugin commands such as status and diagnostics may wrap these executable commands; their exact names should follow the verified plugin conventions.

### 9.4 Local statistics from short-lived hooks

A hook process does not automatically share memory with a persistent aggregator. Do not claim session-wide statistics using an in-process counter that disappears after every tool call.

For v0.1, an optional local service in the same executable can receive small reports over bounded, non-blocking IPC. Start it through an explicit setup/launcher path, not an expensive per-result exporter initialization. Use private local endpoints, cap message sizes, and count drops where observable.

Compression must work without this service. If aggregation is unavailable, show statistics as unavailable or partial, not zero. Offline commands may explicitly write reports to user-selected files.

Keep correctness manifests separate: optional report aggregation cannot become a prerequisite for replay consistency.

## 10. Proxy requirements and protocol roadmap

### 10.1 v0.1: Anthropic Messages

Implement a transparent proxy before adding mutation. Configure a fixed trusted upstream, which may itself be an existing organizational LLM gateway.

Only transform approved text fields in eligible `tool_result` blocks. Keep identifiers, message boundaries, ordering, error flags, cache controls, thinking/signature blocks, tool definitions, and all other content unchanged.

Support streaming and non-streaming requests. Discover and test the additional endpoints the actual client uses; forward supported non-compression routes transparently rather than advertising full client support from one successful Messages request.

### 10.2 HTTP correctness

- Preserve end-to-end headers, status codes, provider errors, rate-limit information, and required beta/version headers.
- Apply normal reverse-proxy handling for hop-by-hop headers; do not promise literal wire-header identity.
- Recompute length-related metadata after a request-body change. Never leave stale integrity signatures or body digests.
- Bypass mutation for signed, opaque, unsupported encoded, or unverifiable payloads.
- Preserve untouched body fragments, including unknown fields, where practical. Avoid serializing the entire provider request just to replace one result string.
- Constrain upstream resolution to configured destinations. Never turn an incoming path, header, or credential into an unrestricted upstream URL.
- Do not add hidden inference retries. Preserve the client's normal retry semantics and avoid duplicate side effects or billing.

### 10.3 Streaming and cancellation

Relay response payloads immediately. Do not accumulate the complete response for logging or token counting. Treat protocol observation as a bounded side path and preserve response content even if observation fails.

Test fragmented SSE frames, arbitrary network chunk boundaries, multiline data, usage events, unknown event types, long streams, errors, and mid-stream disconnects. Distinguish time-to-first-byte from time-to-first-model-content; a heartbeat is not a generated token.

Propagate client cancellation upstream and release work permits and buffers. Do not let abandoned CPU jobs grow without bound.

### 10.4 Bounded request inspection

Limit how much request content is buffered for inspection. When a request exceeds that limit, preserve the buffered prefix and stream the remaining original body unchanged when protocol framing permits. Never discard the prefix, truncate the body, or advertise the compression buffer limit as a provider API limit.

If transparent passthrough cannot be implemented safely for a particular framing/encoding case, document and explicitly handle that case. Maintain independent deployment protection limits for security; a hard request rejection is not the same as a compression bypass.

### 10.5 Later adapters

| Adapter | Scope and extra work |
| --- | --- |
| OpenAI Chat Completions | Supported tool-role content; unknown fields, streaming, usage, tool pairing, and history tests. |
| OpenAI Responses | Explicit input-item handling, tool outputs, reasoning/opaque items, stateful continuation, and previous-response behavior. Do not assume full history is visible. |
| Bedrock / Vertex | Dedicated authentication/signing and endpoint tests; never rewrite signed bodies blindly. |
| Realtime / WebSocket | A separate transport/state design, not an SSE checkbox. |
| Other OpenAI-compatible providers | Per-provider compatibility evidence; similar JSON is not proof of identical behavior. |

A working official-protocol adapter does not establish support for all services that call themselves compatible. Keep a compatibility matrix with tested client/provider versions and known limitations.

## 11. Configuration and rollout policy

Prefer a small versioned TOML configuration. The following defaults are **proposed starting values**, not measured optimal values or provider limits. Validate and revise them transparently during M0/M1.

```toml
config_version = 1
mode = "observe"

[compression]
min_candidate_bytes = 2048
max_candidate_bytes = 2097152
max_json_depth = 64
max_table_rows = 20000
min_estimated_token_savings = 64
min_estimated_savings_ratio = 0.10
processing_budget_ms = 10
max_parallel_jobs = 4

[transforms]
json_minify = true
json_table = false
exact_line_runs = false
line_templates = false

[proxy]
listen = "127.0.0.1:8789"
# Required in proxy mode; explicitly supply a trusted upstream.
# upstream = "https://api.anthropic.com"
inspection_buffer_bytes = 16777216

[telemetry]
content_capture = false
local_reports = true
otlp_enabled = false
queue_capacity = 4096
export_timeout_ms = 2000
shutdown_timeout_ms = 2000
```

`json_table`, `exact_line_runs` and `line_templates` are enabled explicitly only for approved profiles after evaluation. Their default can change in a later release with evidence and release notes.

The processing budget is cooperative, not a promise that cancelling an async waiter terminates blocking CPU work. Enforce byte/depth/work limits and bounded concurrency as well. Never spawn unlimited background work when a time budget expires.

Configuration validation must reject inconsistent thresholds, unknown critical keys, unsafe listener/upstream combinations, and unsupported versions. Environment overrides must be documented and redacted where sensitive. Per-session behavior remains pinned even if defaults change.

Rollout sequence: offline fixtures -> observe mode on representative traffic -> opt-in safe mode for supported new sessions -> broader defaults only after quality and compatibility evidence.

## 12. Observability: v0.1 contract, v0.2 OpenTelemetry export

### 12.1 Adopt the principle, not the whole reference system

Follow the asynchronous instrumentation direction of `sofatutor/llm-proxy`: request processing and telemetry delivery are separate concerns. [S3], [S4]

Deslag does not need a new distributed event platform. Prefer the OpenTelemetry SDK's batching/export facilities where they already solve the problem. Add a small local report sink only for the short-lived-hook and local-statistics requirements that the SDK alone does not address.

Avoid serial subscriber retries that delay unrelated consumers. Use independent bounded paths where necessary, or let a Collector perform downstream fan-out.

### 12.2 Hot-path contract

Allowed: monotonic timestamps, bounded counters, small fixed-category attributes, and non-blocking publication of metadata.

Not allowed for telemetry: network I/O, waiting for queue capacity, synchronous disk persistence, full body copies, unbounded serialization, blocking retries, or force-flushing on every request/tool result.

Queued metadata must not retain references to entire request/response buffers. Bound both queue length and event size. On overflow, drop telemetry, increment an observable drop counter where possible, and continue the user operation.

Use bounded exporter deadlines, retry work, and shutdown drains. Collector outages must degrade observability, not agent operation. Do not instrument exporter traffic back into the same pipeline.

Correctness manifests are a separate exception to optional telemetry work: a required state commit must finish before submission, and its latency must be measured explicitly.

### 12.3 OpenTelemetry design

Provide an optional OTLP exporter in v0.2, with the first supported transport chosen and documented during implementation. Use standard service/resource identity, trace context, and applicable HTTP/GenAI conventions. Resolve and pin the current GenAI convention version rather than assuming historical names remain stable. [S9], [S10]

Proposed trace shape:

```text
incoming proxy request (server span)
  -> request inspection / session consistency
  -> compression (optional child span)
  -> upstream request (client span; covers streaming lifetime)

hook invocation
  -> compression
  -> best-effort local report
```

Record the operation's real start/end timestamps and context; asynchronous export must not create misleading spans timed only around the dispatcher. Preserve valid parentage across async tasks. Do not invent a parent span from a session ID.

Trace propagation to an upstream gateway/provider is an explicit policy. Do not forward arbitrary baggage or secrets. Custom traces must not claim visibility into model execution or tool runtime that deslag did not observe.

Hook-only mode measures hook/compression behavior, not complete provider latency or billing. Proxy mode can additionally observe request/response metadata and provider usage. State these differences in the user interface and documentation.

### 12.4 Initial metrics

Names below are proposed custom instruments, not established OpenTelemetry semantic-convention names. Finalize names, units, temporality, and dimensions in `docs/observability.md`.

| Proposed instrument | Purpose |
| --- | --- |
| `deslag.compression.operations` | Changed, would-change, skipped, and failed counts by bounded category. |
| `deslag.compression.duration` | Transformation latency, in seconds. |
| `deslag.compression.bytes` | Original, candidate, and emitted bytes, clearly separated. |
| `deslag.compression.tokens` | Optional local counts/estimates with explicit method and stage. |
| `deslag.proxy.overhead` | Deslag's measured contribution apart from upstream time. |
| `deslag.telemetry.dropped` | Observable report/span drops by bounded cause. |
| `deslag.telemetry.queue_depth` | Buffer pressure, when supported by the chosen SDK/sink. |
| `deslag.telemetry.export_failures` | Failed exports without payload or credential logging. |
| `deslag.session.consistency_failures` | Missing, incompatible, or conflicting replay state. |

Use standard HTTP and appropriate provider/GenAI usage instruments instead of duplicating equivalent metrics under new names. Record provider cache usage according to the provider's meaning; do not add already-included cached tokens twice.

Keep metric attributes low-cardinality: adapter, bounded transform enum, outcome, skip reason, counting method, and allowlisted model/profile categories. Session IDs, request IDs, arbitrary tool names, paths, raw URLs, content hashes, and user input are not general metric labels.

Counters for operational health should not depend solely on sampled traces. Sampling and exporter loss must be visible when interpreting reports.

### 12.5 Savings accounting

Expose distinct measurements:

1. **Unique representation reduction:** one result's original versus encoded content, counted once where identity is known.
2. **Submitted context reduction:** the estimated/countable difference across actual requests, including repeated history submissions.
3. **Provider-reported usage:** actual returned input/output/cache fields, where observed.
4. **Estimated economic benefit:** an optional calculation with explicit pricing, cache, billing, and counterfactual assumptions.

Observe-mode candidates are hypothetical, not delivered savings. Hook mode cannot infer repeated provider submissions from a single tool-result event. Partial or missing telemetry must not be presented as complete accounting.

Do not label `estimated tokens removed * standard input price` as actual money saved. Caching, extra output, retries, subscription billing, and task behavior can change the net result. Compare complete tasks to determine benefit.

### 12.6 Privacy and retention

Default exports contain no prompts, outputs, tool arguments, raw paths, arbitrary headers, authorization values, query strings, or content-bearing exception text. Use an allowlist of metadata fields rather than attempting to redact an unrestricted dump.

Raw content capture is outside the initial default product. Any later diagnostic mode requires explicit opt-in, retention/size limits, clear destination disclosure, and tests proving it is off by default. Hashing sensitive content does not automatically make it anonymous.

A future original-content retrieval store must have separate permissions and lifecycle guarantees. Droppable telemetry is not reliable original storage.

## 13. Performance engineering and provisional budgets

Measure end-to-end overhead, not just an isolated transform. Compare direct-to-mock, transparent-proxy, observe, and safe modes using the same workload.

### 13.1 Benchmark dimensions

Include candidate sizes of 2 KiB, 16 KiB, 64 KiB, 256 KiB, and 1 MiB; both repetitive and incompressible data; large surrounding histories; and concurrency levels of 1, 8, and 32, subject to documented work limits.

Measure:

- Classification, transformation, gain calculation, and token estimation separately.
- Allocation count/bytes, peak resident memory, CPU, and throughput.
- Whole-hook startup and execution, including cold/warm behavior.
- HTTP request inspection/patching and session-state persistence/replay.
- Streaming first-byte/content delay, cancellation, and sustained relay behavior.
- Telemetry disabled, healthy, saturated, and disconnected.

Record CPU/OS, binary/compiler versions, features, commit, corpus version, tokenizer method, warmup, sample count, and concurrency. Do not compare unmatched configurations or exclude expensive work from the published path.

### 13.2 Proposed initial budgets

These are engineering targets, not performance claims. Confirm them on named reference machines in M0, then freeze release thresholds or document changes in an ADR.

| Case | Initial target |
| --- | --- |
| Supported core transform on <=64 KiB candidate, including local gain estimation | p95 <=3 ms; p99 <=10 ms at concurrency 1. |
| Proxy overhead on <=256 KiB requests with <=64 KiB eligible content | p95 <=10 ms excluding mock upstream delay, but including required session-state work. |
| Warm complete hook invocation on <=64 KiB candidate | p95 <=25 ms on each supported reference platform. |
| Incremental metadata instrumentation overhead in the persistent runtime | p95 <=0.5 ms for the same fixture workload. |
| Collector disconnected under sustained load | No unbounded memory/work growth; incremental p95 request overhead <=1 ms against healthy-export baseline. |

Publish distributions and skip rates, not only favorable medians. Record cold-start results independently; do not hide them behind warm tokenizer or daemon initialization.

### 13.3 Implementation priorities

Start with simple bounded implementations. Optimize from profiles rather than adopting unsafe parsing or custom allocators prematurely.

Use cheap no-op gates, borrowed input where possible, preserved raw request fragments, reused HTTP connections, bounded blocking-work permits, and preinitialized optional tokenizers.

A timeout around an async waiter must not leak CPU work. Keep the concurrency permit until the underlying work actually ends; add cooperative budget checks where possible. Large-input caps are a backstop, not a substitute for cancellation tests.

Do not add a daemon or IPC round trip to compression itself merely to improve telemetry. The local service is optional unless a measured, explicitly documented design proves a broader persistent runtime is needed.

## 14. Test strategy and release gates

### 14.1 Representation correctness

Use example fixtures, property-based tests, and fuzzing where appropriate:

- Valid/invalid/truncated JSON, duplicate keys, excessive depth, mixed schemas, and empty collections.
- Large integers, negative zero, exponents, Unicode/escapes, and embedded control characters.
- Field/row order, null versus missing, and wrapper collisions.
- Exact repetition reconstruction across LF/CRLF, empty lines, and final-line variants.
- Byte-sensitive bypasses and unknown content.
- Idempotence, bounded output, and absence of partial transformation on failure.

All selected values and required metadata must be preserved. Generated fixtures are necessary but not sufficient; include sanitized real result shapes.

### 14.2 Integration and replay correctness

Test actual adapter envelopes against a mock provider and supported client versions:

- Tool-call/result pairing, multiple content blocks, unknown fields, errors, media, and non-text MCP content.
- Parallel tools, subagents, forks, repeated submissions, and existing compression owners.
- Observe-to-safe transitions, disabling, model/profile changes, and version upgrades.
- Restart recovery, missing/corrupt manifests, state limits, unavailable disk, and metadata expiration.
- Previously skipped results remaining skipped across later submissions.
- Cached-prefix stability and applicable thinking/signature preservation.
- Hook replacement rejection/conflict and unsupported client versions.
- Stream fragmentation, cancellations, provider errors, and usage extraction failures.
- Upstream credential isolation, header preservation, and no unexpected network destinations.

For future Responses support, test both explicitly supplied histories and provider-side continuation. OpenAI documents response chaining through `previous_response_id`; a proxy cannot assume every incoming request contains the full earlier conversation. [S11]

### 14.3 Fault injection

Exercise full compression queues, parser errors, worker panics, large bodies, blocked/disconnected collectors, slow report consumers, IPC failure, disk failure, and abrupt restarts.

Expected outcomes must distinguish:

- New-result compression failure: unchanged content and an observable bypass where possible.
- Telemetry failure: original operation continues; accounting may be partial.
- Established-history inconsistency: explicit recovery/error behavior, never silent history mutation.

### 14.4 Agent effectiveness

Build paired task evaluations using original and compressed variants of the same representative work. Use identical task inputs, model settings, tool environments, and scoring rules. Randomize run order and use session/task-level analysis rather than treating repeated turns as independent samples.

Cover structured API querying, code-search result interpretation, failure diagnosis, issue triage, logs, aggregation over rows, exact-ID lookup, and tasks with important evidence in the middle or end of a large result.

Measure task success, exact-value/evidence errors, tool-call count, retries, input/output/cache usage when observed, and total wall-clock time. Include incompressible and negative-control fixtures. Report per-task-category results, not just a favorable overall average.

Proposed default-enablement gate for structural encodings:

- Zero known protocol, data-preservation, or session-consistency violations in the release corpus.
- No unresolved critical evidence-loss or incorrect-action regression.
- A predeclared paired non-inferiority analysis for task success. Starting policy: lower one-sided 95% confidence bound on compressed-minus-baseline success must exceed -2 percentage points.
- Beneficial total usage/context results on the intended workload, without an unacceptable tool-call or wall-clock regression.

The confidence criterion may need substantially more tasks than a small pilot. A small inconclusive sample is not proof of equivalence: keep the transform opt-in, broaden evaluation, or narrow the supported profile. Do not lower the criterion after inspecting results without an explicit decision.

Live evaluations require authorized credentials and a spending cap. Offline CI must not unexpectedly invoke paid models or export real user content.

### 14.5 Automated quality checks

Run formatting, linting, locked dependency builds, unit/property/integration tests, secret scanning, dependency/license checks, and release smoke tests. Add scheduled fuzzing and controlled-runner benchmarks as the project matures.

Once the workspace exists, expected baseline commands include:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Do not mark commands as run until they have actually completed. Add feature-matrix and platform-specific checks according to the implemented workspace.

## 15. Security, licensing, and supply-chain requirements

Bind local services to loopback or private IPC by default. Loopback is not an authentication mechanism: protect administrative/state endpoints with a local capability or appropriate OS permissions, reject unexpected browser-origin access, and prevent cross-user reads.

Treat the proxy as a credential-bearing service. Do not log secrets, dynamically trust upstream destinations from clients, disable TLS verification, or silently forward privileged trace baggage. A later network deployment needs explicit authentication and tenant isolation; do not expose desktop defaults publicly.

Require checksums, release provenance, pinned dependencies/toolchains, private local state, and no arbitrary downloads from runtime hooks. Build publication actions must not be available to untrusted pull-request code with release credentials.

Before reusing Headroom or `llm-proxy` source, inspect the exact pinned revision's license and notices. Record copied/adapted files and preserve required notices and attribution. Choosing a project license and verifying reuse obligations are explicit release tasks; this plan is not a substitute for that review.

The public product name is **deslag** (see [ADR 0002](decisions/0002-rename-to-deslag.md)). The crates.io name `deslag` belongs to an unrelated crate, so publish crates as `deslag-*` and document `cargo install deslag-cli`. Do not rename the project again without a decision record.

## 16. Ordered implementation backlog

All items begin pending. Update status only with implementation and verification evidence.

### M0 - Compatibility spike and architectural proof

**Goal:** Prove the interception and state boundaries before committing to a broad implementation.

| ID | Task | Acceptance evidence |
| --- | --- | --- |
| DF-001 | Inspect repository and reference revisions; initialize minimal Rust workspace and CI if needed. | Repository inventory; pinned toolchain; license/reuse notes; successful locked build/test. |
| DF-002 | Build no-op Claude plugin/hook adapter. | Real supported-client invocation; captured sanitized schemas; accepted and rejected replacement tests; minimum tested version recorded. |
| DF-003 | Build transparent Anthropic proxy against a mock upstream. | Request/header/status fidelity; streaming and cancellation tests; no credential leakage. |
| DF-004 | Resolve session identity, ownership, replay, resume, and upgrade behavior. | ADR with a working multi-turn/restart experiment; explicit unsupported cases. |
| DF-005 | Add one JSON-minification slice through both adapters and establish benchmark runners. | Same core used by hook and proxy; no-op/changed fixtures; measured baseline; no historical drift. |

**Exit:** There is a demonstrated path from supported tool result to provider-visible representation. Compatibility claims are limited to observed behavior. Where live access is unavailable, the affected release gate remains pending; continue offline core work without pretending the gate passed.

### M1 - Compression core and evidence

**Goal:** Implement the high-leverage deterministic transformations independently of integrations.

| ID | Task | Depends on | Acceptance evidence |
| --- | --- | --- | --- |
| DF-010 | Finalize core contract, reports, policy modes, limits, and skip enums. | DF-005 | Schema tests; no-op borrowing where practical; observe-mode accounting. |
| DF-011 | Harden lexical-preserving JSON minification. | DF-010 | Edge-case, property, malformed-input, and lexical-value tests. |
| DF-012 | Implement table-v1 encoder/decoder and eligibility checks. | DF-010 | Format specification; round-trip corpus; gain checks; opt-in policy. |
| DF-013 | Implement exact line-run codec and decoder. | DF-010 | Byte reconstruction across line-ending cases; gain checks; opt-in policy. |
| DF-014 | Implement local counting/estimation interface and calibration fixtures. | DF-010 | Methods labeled; unknown counts handled; no production counting API calls. |
| DF-015 | Add bounded execution, fuzz targets, and benchmark cases. | DF-011/012/013/014 | Allocation/memory results; limits and cancellation behavior; reproducible runs. |
| DF-016 | Build paired task-evaluation harness and initial corpus. | DF-011/012/013 | Scoring rules, cost controls, negative controls, documented results or pending live runs. |

**Exit:** The core has explicit data-preservation guarantees and measured resource behavior. Structural transforms are not made default merely because they pass decoder tests.

### M2 - v0.1 product release

**Goal:** A usable native Claude Code plugin, a minimal supported proxy, and truthful local statistics.

| ID | Task | Depends on | Acceptance evidence |
| --- | --- | --- | --- |
| DF-020 | Integrate production hook schema/field routing. | DF-002, M1 | Built-in/MCP allowlist tests; parallel/subagent handling; no stdout contamination. |
| DF-021 | Implement replay manifests and ownership enforcement. | DF-004, DF-010 | Resume/restart/skip/disable/model-switch tests; required state commits measured. |
| DF-022 | Add selective Anthropic request mutation. | DF-003, DF-021, M1 | Protected-block fidelity; stable prefixes; large-body bypass; transparent streaming. |
| DF-023 | Implement config, launcher, doctor, status, and disable workflow. | DF-020/021/022 | Explicit routing; no global surprise edits; redacted diagnostics; fresh-session guidance. |
| DF-024 | Implement optional local report aggregation and stats. | DF-010 | Bounded IPC; meaningful cross-hook aggregation; disconnected behavior; partial-data labels. |
| DF-025 | Run compatibility, security, fault, performance, and quality gates. | DF-016, DF-020 through DF-024 | Reproducible artifacts; named client/model/platform coverage; unresolved limits documented. |
| DF-026 | Package plugin and native releases; finish onboarding/reference docs. | DF-025 | Clean-machine install; checksum/provenance checks; uninstall/rollback smoke tests. |

**Exit:** v0.1 can be installed and used without a compiler or compression service. Observe mode works; approved safe transforms reduce eligible context; streaming and history remain correct. Table/repetition support can remain opt-in where statistical evidence is incomplete.

### M3 - v0.2 observability and broader protocol support

**Goal:** Optional production-grade telemetry and additional verified harness integration paths.

| ID | Task | Depends on | Acceptance evidence |
| --- | --- | --- | --- |
| DF-030 | Add optional OpenTelemetry metrics/traces and OTLP export. | M2 | Pinned conventions/SDK; context/timing tests; no payload capture by default. |
| DF-031 | Add Collector example and telemetry outage/load tests. | DF-030 | Queue/drop/export metrics; bounded shutdown; no meaningful outage-induced request degradation. |
| DF-032 | Add OpenAI Chat Completions adapter. | DF-021, DF-022 | Provider-specific fixtures, streaming, tool pairing, usage, and replay tests. |
| DF-033 | Add OpenAI Responses adapter. | DF-021, DF-022 | Explicit stateful/stateless coverage; protected opaque items; continuation tests. |
| DF-034 | Add cache-aware usage reports and container distribution. | DF-030 through DF-033 | Estimates separated from actual usage; deployed smoke tests; documented network-security requirements. |

**Exit:** Export backends remain optional. Protocol support is explicit and verified. Deslag can sit in front of an existing gateway without replacing its credential-management or observability platform.

### M4 - v0.3+ evidence-driven expansion

DF-040 is a research/decision item, not an implementation mandate. Evaluate selected tool-specific policies, source/code/diff handling, and optional retrieval-backed compression only after usage data identifies the next worthwhile lever.

Before adding retrieval, define original retention, durability, tenant/session isolation, reconstruction semantics, tool exposure, failure behavior, and net cost including retrieval calls. No lossy offload may depend on a best-effort telemetry queue.

Do not add cross-agent memory, model routing, or self-improving behavior merely because upstream Headroom supports broader features.

## 17. Decisions that must be resolved explicitly

| Decision | Default direction | Required evidence / deadline |
| --- | --- | --- |
| Rust versus Go | Rust | Change only with a written deployment, compatibility, or measured performance reason. |
| Minimum Claude Code version and tool coverage | Tested native result hooks; narrow allowlist | Real-client proof in M0; record exact version. |
| Generic proxy session identity and persistence | Scoped identity plus deterministic replay metadata | Multi-turn/restart proof in DF-004 before generic mutation claims. |
| Default-enabled encodings | JSON minification first; structural/repetition opt-in | Quality and gain results before changing defaults. |
| Exact versus estimated token counts | Explicit per-model method; no hot-path API calls | Calibration and method labels in DF-014. |
| Small-process aggregation | Optional local service using bounded IPC | Benchmarks and failure tests in DF-024. |
| OTLP transport and Rust SDK versions | Small supported standard stack | Compatibility/protocol check in DF-030; pin versions. |
| Project license and source reuse | Focused implementation with recorded provenance | Review before copying source and before publication. |
| Package/binary registry naming | Product is deslag; crates are `deslag-*` (ADR 0002) | Re-check namespaces before release. |
| Additional deployment targets | Only tested platforms | Clean-install smoke tests before listing support. |

## 18. Definition of done for the first release

- [ ] A supported Claude Code user can install and run deslag without building native code.
- [ ] The same core is exercised by the plugin and the minimal Anthropic proxy.
- [ ] Each advertised integration has recorded versioned compatibility evidence.
- [ ] New unsupported content passes through without corruption or truncation.
- [ ] Tool arguments, instructions, protected blocks, media, and unknown fields remain unchanged.
- [ ] Session replay, restarts, skipped-result pinning, and compression ownership are tested.
- [ ] Observe-mode proposals are distinguishable from actual emitted reductions.
- [ ] Local token estimates are labeled; provider usage is not inferred from hook observations.
- [ ] No mandatory external service, local model, SQL database, or distributed event bus exists.
- [ ] Optional telemetry failures do not block compression, tools, or streaming.
- [ ] Performance results include counting, adapter startup, state work, and memory behavior.
- [ ] Default-enabled transforms pass representation, protocol, and appropriate task-quality gates.
- [ ] Privacy defaults, credential routing, packaging provenance, and license obligations are verified.
- [ ] README, compatibility matrix, format specification, configuration, disable/recovery guidance, and benchmark/evaluation methodology are present.
- [ ] Release notes state limitations and distinguish implemented, tested, experimental, and deferred features.

## 19. Required agent completion report

For every milestone, report:

```text
Completed backlog IDs:
Files/components changed:
Tests actually run and results:
Benchmarks actually run and results:
Live client/provider checks actually performed:
Known limitations and failed/pending gates:
Architecture deviations and their rationale:
Next executable backlog item:
```

Never claim the plan is fully implemented from a scaffolding commit or an offline unit test. Separate a working prototype from a supported release.

## 20. Reference material

The primary sources below were used in the planning conversation. Claude/OTel/OpenAI documentation was checked for this handoff on 2026-10-10. Repository source was reviewed earlier in the same conversation; pin and inspect current revisions before borrowing implementation details. Design decisions and numeric budgets in this plan are proposals, not claims made by these sources.

| Ref | Source | Why it matters |
| --- | --- | --- |
| S1 | [Headroom repository][S1] | Product reference; inspect scope without adopting all features. |
| S2 | [Headroom Rust workspace][S2] | Existing native core/proxy; inspect transform modules separately when evaluating reuse. |
| S3 | [llm-proxy instrumentation architecture][S3] | User's reference architecture for async instrumentation and dispatch. |
| S4 | [llm-proxy in-memory event bus][S4] | Concrete publication/drop/fan-out behavior to learn from. |
| S5 | [Claude Code hooks reference][S5] | Result-replacement schema, hook timing, and telemetry caveat. |
| S6 | [Claude Code plugin manifest reference][S6] | Supported manifests, components, and restricted plugin settings. |
| S7 | [Claude Code gateway connection][S7] | Explicit provider routing configuration. |
| S8 | [Anthropic prompt caching][S8] | Prefix stability and provider-specific cache behavior. |
| S9 | [OpenTelemetry tracing SDK specification][S9] | Synchronous callbacks, batching, bounded queues, and shutdown/export behavior. |
| S10 | [OpenTelemetry GenAI conventions entry point][S10] | Resolve the current canonical conventions and pin their version; the entry point may direct to a newer repository. |
| S11 | [OpenAI conversation state][S11] | Responses continuation and why a proxy may not see the entire history. |

Additional source path inspected in the initial discussion: `headroomlabs-ai/headroom/crates/headroom-core/src/transforms/mod.rs`. Treat its transform inventory as research input, not a requirement for parity.

[S1]: https://github.com/headroomlabs-ai/headroom
[S2]: https://github.com/headroomlabs-ai/headroom/blob/main/Cargo.toml
[S3]: https://github.com/sofatutor/llm-proxy/blob/main/docs/observability/instrumentation.md
[S4]: https://github.com/sofatutor/llm-proxy/blob/main/internal/eventbus/eventbus.go
[S5]: https://code.claude.com/docs/en/hooks
[S6]: https://code.claude.com/docs/en/plugins-reference
[S7]: https://code.claude.com/docs/en/llm-gateway-connect
[S8]: https://platform.claude.com/docs/en/build-with-claude/prompt-caching
[S9]: https://opentelemetry.io/docs/specs/otel/trace/sdk/
[S10]: https://opentelemetry.io/docs/specs/semconv/gen-ai/
[S11]: https://developers.openai.com/api/docs/guides/conversation-state

---

**Implementation priority:** prove the integration boundary, ship deterministic structured-result compression, preserve conversation stability, measure the complete task, and keep observability out of the critical path.
