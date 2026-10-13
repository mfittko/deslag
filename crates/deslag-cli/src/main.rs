//! `deslag` command-line entry point.

mod codec;
mod hook_conflicts;

use std::cell::RefCell;
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::SystemTime;

use deslag_core::{BoundedExecutor, CompressionReport, FIRST_SLICE_JSON_MINIFY_CONTEXT, Rejected};
use deslag_proxy::session::{self, Mode, Owner, Store};
use deslag_proxy::{Upstream, serve};
use deslag_telemetry as report;
use serde_json::{Value, json};
use tokio::net::TcpListener;

const USAGE: &str = "usage: deslag hook claude [--minify-json]\n       \
                     deslag proxy --upstream <url> [--listen <addr>] [--minify-json]";
const PROXY_USAGE: &str = "usage: deslag proxy --upstream <url> [--listen <addr>] [--minify-json]";

// Exit 2 from a Claude Code hook is blocking and feeds stderr to the model; usage errors avoid it.
const EX_USAGE: u8 = 64;
const EX_DATAERR: u8 = 65;

// ponytail: fixed 16 MiB ceiling on hook input; larger payloads pass through untouched.
// Make it configurable when real tool outputs routinely exceed it.
const MAX_HOOK_INPUT_BYTES: u64 = 16 * 1024 * 1024;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["hook", "claude", flags @ ..] if matches!(flags, [] | ["--minify-json"]) => {
            let exec = BoundedExecutor::default();
            let env = |key: &str| std::env::var_os(key);
            let (input, minify_json) = (io::stdin().lock(), !flags.is_empty());
            match claude_hook(
                input,
                minify_json,
                hook_owns_compression,
                env,
                &exec,
                minify,
            ) {
                // A failed write leaves the tool output unchanged, which is the safe default.
                Ok(Some(output)) => drop(writeln!(io::stdout().lock(), "{output}")),
                Ok(None) => {}
                Err(message) => eprintln!("deslag: {message}"),
            }
            ExitCode::SUCCESS
        }
        // Experimental mod bridge; see codec.rs. Not part of the documented CLI.
        ["codec", "--protocol", "1"] => {
            let owner = || codec::owner_is_mod(session::state_dir().as_deref());
            let exec = BoundedExecutor::default();
            match codec::run(
                io::stdin().lock(),
                MAX_HOOK_INPUT_BYTES,
                owner,
                &exec,
                minify,
            ) {
                Ok(line) => match writeln!(io::stdout().lock(), "{line}") {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(_) => ExitCode::from(1),
                },
                Err(message) => {
                    eprintln!("deslag: {message}");
                    ExitCode::from(EX_DATAERR)
                }
            }
        }
        ["proxy", args @ ..] => match proxy(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("deslag: {message}");
                ExitCode::from(2)
            }
        },
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(EX_USAGE)
        }
    }
}

/// Runs the proxy until it fails. The tokio runtime exists only here, so
/// `hook claude` stays synchronous.
fn proxy(args: &[&str]) -> Result<(), String> {
    let mut listen = "127.0.0.1:8789";
    let mut upstream = None;
    let mut minify_json = false;
    let mut args = args.iter().copied();
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .ok_or(format!("{arg} needs a value\n{PROXY_USAGE}"))
        };
        match arg {
            "--listen" => listen = value()?,
            "--upstream" => upstream = Some(value()?),
            "--minify-json" => minify_json = true,
            _ => return Err(format!("unknown argument: {arg}\n{PROXY_USAGE}")),
        }
    }
    let mut upstream = Upstream::parse(upstream.ok_or(PROXY_USAGE)?)?;
    if minify_json {
        let dir = session::state_dir().ok_or(
            "--minify-json needs a state directory: set DESLAG_STATE_DIR, XDG_STATE_HOME or HOME",
        )?;
        session::prune(&dir, session::RETENTION, SystemTime::now())
            .map_err(|e| format!("cannot prune session state in {}: {e}", dir.display()))?;
        let store = Store {
            dir,
            mode: Mode::Safe,
        };
        upstream = upstream.with_json_minification(store);
    }
    let addr: SocketAddr = listen
        .parse()
        .map_err(|e| format!("invalid --listen {listen}: {e}"))?;
    // PLAN §15: desktop defaults bind loopback only; network exposure needs authentication first.
    if !addr.ip().is_loopback() {
        return Err(format!("--listen must be a loopback address, got {addr}"));
    }
    let runtime =
        tokio::runtime::Runtime::new().map_err(|e| format!("cannot start async runtime: {e}"))?;
    runtime.block_on(async {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| format!("cannot listen on {addr}: {e}"))?;
        let bound = listener.local_addr().map_err(|e| e.to_string())?;
        eprintln!("deslag proxy listening on {bound}, upstream {upstream}");
        serve(listener, upstream).await.map_err(|e| e.to_string())
    })
}

/// Validates a Claude Code PostToolUse event. With `minify_json`, returns the hook
/// output that replaces minifiable JSON in a Bash stdout or MCP text block. `None`
/// leaves the tool output unchanged. The hook keeps no per-result state: each call
/// reads only its own event and the session owner, so parallel tools and subagents
/// cannot affect each other.
///
/// Errors are diagnostics only. The caller reports them on stderr and still exits 0,
/// so the hook never blocks the tool. A rejected or panicked compression job is
/// such an error (`compression <reason>; tool output unchanged`), and it leaves
/// every routed field unchanged. Another `PostToolUse` hook that may replace
/// the same output is such an error, because the client keeps only the last write.
///
/// `owns_compression` is the session ownership gate. Every routed field passes
/// through it, and it is checked only with `minify_json`.
fn claude_hook(
    mut input: impl Read,
    minify_json: bool,
    owns_compression: impl FnOnce() -> bool,
    env: impl Fn(&str) -> Option<OsString>,
    exec: &BoundedExecutor,
    job: Job,
) -> Result<Option<Value>, String> {
    let mut buf = Vec::new();
    input
        .by_ref()
        .take(MAX_HOOK_INPUT_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|err| format!("cannot read hook input: {err}"))?;
    if buf.len() as u64 > MAX_HOOK_INPUT_BYTES {
        // Drain the rest so the client never sees a broken pipe on its write.
        let _ = io::copy(&mut input, &mut io::sink());
        return Err(format!(
            "hook input exceeds {MAX_HOOK_INPUT_BYTES} bytes; passing through"
        ));
    }
    let event: Value =
        serde_json::from_slice(&buf).map_err(|err| format!("malformed hook input: {err}"))?;
    match event.get("hook_event_name").and_then(Value::as_str) {
        Some("PostToolUse") => {}
        Some(other) => return Err(format!("unsupported hook event: {other:?}")),
        None => return Err("hook input has no hook_event_name".to_string()),
    }
    let Some(tool_name) = event.get("tool_name").and_then(Value::as_str) else {
        return Err("hook input has no tool_name".to_string());
    };
    // MCP tools return strings or content-block arrays, so any non-null value is valid.
    if event.get("tool_response").is_none_or(Value::is_null) {
        return Err("hook input has no tool_response".to_string());
    }
    // With any owner other than the hook, no routed field is replaced.
    if !minify_json || !owns_compression() {
        return Ok(None);
    }
    // Only Bash stdout and MCP text blocks are eligible. Every other tool, including
    // Read, stays byte-exact because Edit matches on Read output.
    let response = &event["tool_response"];
    let tool = report::ToolCategory::from_tool_name(tool_name);
    let socket = report::live_socket();
    // Reports wait until every job returns, so an output with a rejected job
    // reports nothing, and a job that outlives its budget never reports.
    let reports = RefCell::new(Vec::new());
    // Each text runs as its own bounded job. Any rejection leaves the whole output unchanged.
    let run = |text: &str| {
        let owned = text.to_owned();
        let (changed, mut compressed) = exec.run(move || job(&owned))?;
        if socket.is_some() {
            report::add_estimates(&mut compressed, text, changed.as_deref().unwrap_or(text));
            reports.borrow_mut().push(compressed);
        }
        Ok(changed)
    };
    let updated = match tool {
        report::ToolCategory::Bash => minify_bash(response, &run),
        report::ToolCategory::Mcp => minify_mcp(response, &run),
        report::ToolCategory::Other => Ok(None),
    }
    .map_err(|rejected| format!("compression {}; tool output unchanged", rejected.as_str()))?;
    if let Some(socket) = &socket {
        // ponytail: a hook process exits after one result, so a failed send is lost uncounted.
        for compressed in reports.into_inner() {
            report::send_report(socket, &compressed, report::Adapter::Hook, tool);
        }
    }
    let Some(output) = updated.and_then(|updated| replacement(&event, updated)) else {
        return Ok(None);
    };
    // An empty variable counts as unset, so the lookup falls through to the next source.
    let env = |key: &str| env(key).filter(|value| !value.is_empty());
    let config_dir = env("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|home| PathBuf::from(home).join(".claude")));
    let project_dir = env("CLAUDE_PROJECT_DIR")
        .map(PathBuf::from)
        .or_else(|| event["cwd"].as_str().map(PathBuf::from));
    match hook_conflicts::find(tool_name, config_dir.as_deref(), project_dir.as_deref()) {
        Some(conflict) => Err(conflict),
        None => Ok(Some(output)),
    }
}

/// One finished job: the changed text, if any, and the compression report.
type Compressed = (Option<String>, CompressionReport);

/// Per-text compression job. Tests inject a slow one.
type Job = fn(&str) -> Compressed;

/// Runs one job for one text under the executor.
type Run<'a> = dyn Fn(&str) -> Result<Option<String>, Rejected> + 'a;

fn minify(text: &str) -> Compressed {
    let result = deslag_core::compress(text, &FIRST_SLICE_JSON_MINIFY_CONTEXT);
    let report = result.report.clone();
    (result.into_changed(), report)
}

/// Bash: only `stdout`, and never for image results.
fn minify_bash(response: &Value, run: &Run) -> Result<Option<Value>, Rejected> {
    if response["isImage"] == true {
        return Ok(None);
    }
    let Some(stdout) = response.get("stdout").and_then(Value::as_str) else {
        return Ok(None);
    };
    let Some(minified) = run(stdout)? else {
        return Ok(None);
    };
    let mut updated = response.clone();
    updated["stdout"] = minified.into();
    Ok(Some(updated))
}

/// MCP: only the `text` of `"type": "text"` blocks in a content-block array.
fn minify_mcp(response: &Value, run: &Run) -> Result<Option<Value>, Rejected> {
    let Some(blocks) = response.as_array() else {
        return Ok(None);
    };
    let mut updated = blocks.clone();
    let mut changed = false;
    for block in updated.iter_mut().filter(|block| block["type"] == "text") {
        let Some(text) = block["text"].as_str() else {
            continue;
        };
        if let Some(minified) = run(text)? {
            block["text"] = minified.into();
            changed = true;
        }
    }
    Ok(changed.then_some(Value::Array(updated)))
}

/// The hook compresses only when the configured owner is the hook. It keeps no
/// replay state: the client stores and resends the replacement. The owner is
/// read per call, so after a mid-session change from `proxy` to `hook` the hook
/// compresses new results while the proxy replays its earlier decisions.
fn hook_owns_compression() -> bool {
    session::state_dir().is_some_and(|dir| session::read_owner(&dir) == Owner::Hook)
}

/// Builds the PostToolUse output that replaces the tool result with `updated`.
///
/// Claude Code drops an `updatedToolOutput` that does not match the tool's output
/// schema, so this returns `None` unless `updated` has the same shape as the
/// original `tool_response`.
fn replacement(event: &Value, updated: Value) -> Option<Value> {
    if !same_shape(event.get("tool_response")?, &updated) {
        return None;
    }
    Some(json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "updatedToolOutput": updated,
        }
    }))
}

/// Same JSON type; objects need the same key set and arrays the same length,
/// with same-shaped values at each key or index.
fn same_shape(original: &Value, updated: &Value) -> bool {
    match (original, updated) {
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| same_shape(value, other)))
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_shape(x, y))
        }
        (Value::Null, Value::Null)
        | (Value::Bool(_), Value::Bool(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::String(_), Value::String(_)) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!(
            "{}/../../tests/fixtures/claude/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("deslag-main-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A budget no debug build on a slow runner can miss, so tests that expect a
    /// replacement never see a fail-open rejection.
    fn generous() -> BoundedExecutor {
        BoundedExecutor::new(4, std::time::Duration::from_secs(60)).unwrap()
    }

    /// [`claude_hook`] with the production job under [`generous`].
    fn claude(
        input: &[u8],
        minify_json: bool,
        env: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Option<Value>, String> {
        claude_hook(input, minify_json, || true, env, &generous(), minify)
    }

    /// Runs the hook with config and project dirs at `dir`, so real settings never apply.
    fn hook_in(dir: &std::path::Path, input: &[u8], minify: bool) -> Result<Option<Value>, String> {
        claude(input, minify, |key| {
            matches!(key, "CLAUDE_CONFIG_DIR" | "CLAUDE_PROJECT_DIR")
                .then(|| dir.as_os_str().to_owned())
        })
    }

    fn hook(input: &[u8], minify: bool) -> Result<Option<Value>, String> {
        let missing = std::env::temp_dir().join(format!("deslag-main-{}-none", std::process::id()));
        hook_in(&missing, input, minify)
    }

    fn minified(event: &Value) -> Option<Value> {
        let bytes = serde_json::to_vec(event).unwrap();
        hook(&bytes, true)
            .unwrap()
            .map(|output| output["hookSpecificOutput"]["updatedToolOutput"].clone())
    }

    const PRETTY: &str = "{\n  \"a\": [\n    1,\n    2\n  ]\n}";
    const COMPACT: &str = r#"{"a":[1,2]}"#;

    #[test]
    fn valid_bash_replacement_is_accepted() {
        let event = fixture("posttooluse-bash.json");
        let mut updated = event["tool_response"].clone();
        updated["stdout"] = json!("compressed");

        let output = replacement(&event, updated.clone()).expect("replacement accepted");

        assert_eq!(
            output,
            json!({
                "hookSpecificOutput": {
                    "hookEventName": "PostToolUse",
                    "updatedToolOutput": updated,
                }
            })
        );
        let text = serde_json::to_string(&output).unwrap();
        assert!(text.contains(r#""updatedToolOutput":{"#));
        assert!(text.contains(r#""stderr":"""#));
    }

    #[test]
    fn bare_string_for_bash_is_rejected() {
        let event = fixture("posttooluse-bash.json");
        assert_eq!(replacement(&event, json!("compressed")), None);
    }

    #[test]
    fn object_missing_a_key_is_rejected() {
        let event = fixture("posttooluse-bash.json");
        let mut updated = event["tool_response"].clone();
        updated.as_object_mut().unwrap().remove("stderr");
        assert_eq!(replacement(&event, updated), None);
    }

    #[test]
    fn nested_type_change_is_rejected() {
        let event = fixture("posttooluse-read.json");
        let mut updated = event["tool_response"].clone();
        updated["file"]["numLines"] = json!("2");
        assert_eq!(replacement(&event, updated), None);
    }

    #[test]
    fn content_block_arrays_compare_elements() {
        let event = json!({"tool_response": [{"type": "text", "text": "long"}]});
        let same = json!([{"type": "text", "text": "short"}]);
        assert!(replacement(&event, same).is_some());
        for bad in [
            json!([{"type": "text", "body": "short"}]),
            json!([{"type": "text", "text": 1}]),
            json!(["short"]),
            json!([]),
            json!([{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]),
        ] {
            assert_eq!(replacement(&event, bad.clone()), None, "{bad}");
        }
    }

    #[test]
    fn valid_events_pass() {
        for name in ["posttooluse-bash.json", "posttooluse-read.json"] {
            let bytes = serde_json::to_vec(&fixture(name)).unwrap();
            assert_eq!(hook(bytes.as_slice(), false), Ok(None));
        }
    }

    #[test]
    fn oversize_input_is_rejected() {
        let bytes = vec![b' '; MAX_HOOK_INPUT_BYTES as usize + 1];
        assert!(
            hook(bytes.as_slice(), false)
                .unwrap_err()
                .contains("exceeds")
        );
    }

    #[test]
    fn input_at_the_cap_is_read() {
        let bytes = vec![b' '; MAX_HOOK_INPUT_BYTES as usize];
        assert!(
            hook(bytes.as_slice(), false)
                .unwrap_err()
                .contains("malformed")
        );
    }

    #[test]
    fn wrong_event_and_missing_fields_are_rejected() {
        for input in [
            r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_response":{}}"#,
            r#"{"tool_name":"Bash","tool_response":{}}"#,
            r#"{"hook_event_name":"PostToolUse","tool_response":{}}"#,
            r#"{"hook_event_name":"PostToolUse","tool_name":"Bash"}"#,
            r#"{"hook_event_name":"PostToolUse","tool_name":"Bash","tool_response":null}"#,
        ] {
            assert!(hook(input.as_bytes(), false).is_err(), "{input}");
        }
    }

    #[test]
    fn non_object_tool_responses_pass() {
        for input in [
            r#"{"hook_event_name":"PostToolUse","tool_name":"mcp__x__y","tool_response":"x"}"#,
            r#"{"hook_event_name":"PostToolUse","tool_name":"mcp__x__y","tool_response":[{"type":"text","text":"x"}]}"#,
        ] {
            assert_eq!(hook(input.as_bytes(), true), Ok(None), "{input}");
        }
    }

    #[test]
    fn bash_routes_only_stdout() {
        let mut event = fixture("posttooluse-bash.json");
        event["tool_response"]["stdout"] = PRETTY.into();
        event["tool_response"]["stderr"] = PRETTY.into();
        event["tool_response"]["extra"] = PRETTY.into();
        let mut expected = event["tool_response"].clone();
        expected["stdout"] = COMPACT.into();
        assert_eq!(minified(&event), Some(expected));
    }

    #[test]
    fn untouched_numbers_are_emitted_byte_exact() {
        let input = format!(
            r#"{{"hook_event_name":"PostToolUse","tool_name":"Bash","tool_response":{{"stdout":{},"big":123456789012345678901234567890,"neg":-98765432109876543210,"dec":0.100000000000000000000000000001}}}}"#,
            serde_json::to_string(PRETTY).unwrap()
        );
        let output =
            serde_json::to_string(&hook(input.as_bytes(), true).unwrap().unwrap()).unwrap();
        for number in [
            r#""big":123456789012345678901234567890"#,
            r#""neg":-98765432109876543210"#,
            r#""dec":0.100000000000000000000000000001"#,
        ] {
            assert!(output.contains(number), "{number} in {output}");
        }
    }

    #[test]
    fn bash_image_result_is_skipped() {
        let mut event = fixture("posttooluse-bash.json");
        event["tool_response"]["stdout"] = PRETTY.into();
        event["tool_response"]["isImage"] = true.into();
        assert_eq!(minified(&event), None);
    }

    #[test]
    fn subagent_bash_is_routed_like_main_agent() {
        let mut event = fixture("posttooluse-subagent-bash.json");
        event["tool_response"]["stdout"] = PRETTY.into();
        let mut expected = event["tool_response"].clone();
        expected["stdout"] = COMPACT.into();
        assert_eq!(minified(&event), Some(expected));
    }

    #[test]
    fn mcp_text_blocks_are_minified() {
        let event = fixture("posttooluse-mcp-text.json");
        assert_eq!(
            minified(&event),
            Some(json!([{"type": "text", "text": r#"{"a":1,"b":[1,2]}"#}]))
        );
    }

    #[test]
    fn mcp_routes_only_text_of_text_blocks() {
        let mut event = fixture("posttooluse-mcp-mixed.json");
        event["tool_response"][1]["text"] = PRETTY.into();
        event["tool_response"][1]["annotations"] = json!({"note": PRETTY});
        event["tool_response"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type": "resource", "text": PRETTY}));
        let mut expected = event["tool_response"].clone();
        expected[1]["text"] = COMPACT.into();
        assert_eq!(minified(&event), Some(expected));
    }

    #[test]
    fn mcp_compact_text_is_left_alone() {
        assert_eq!(minified(&fixture("posttooluse-mcp-mixed.json")), None);
    }

    #[test]
    fn mcp_non_array_responses_pass() {
        let mut event = fixture("posttooluse-mcp-text.json");
        for response in [json!(PRETTY), json!({"type": "text", "text": PRETTY})] {
            event["tool_response"] = response;
            assert_eq!(minified(&event), None);
        }
    }

    #[test]
    fn other_tools_are_never_replaced() {
        let mut event = fixture("posttooluse-read.json");
        event["tool_response"]["file"]["content"] = PRETTY.into();
        assert_eq!(minified(&event), None);
        for tool in ["Read", "Edit", "Agent", "ToolSearch", "WebFetch", "bash"] {
            event["tool_name"] = tool.into();
            event["tool_response"] = json!({"stdout": PRETTY});
            assert_eq!(minified(&event), None, "{tool}");
            event["tool_response"] = json!([{"type": "text", "text": PRETTY}]);
            assert_eq!(minified(&event), None, "{tool}");
        }
    }

    #[test]
    fn no_routed_field_is_replaced_unless_the_hook_owns_compression() {
        let mut bash = fixture("posttooluse-bash.json");
        bash["tool_response"]["stdout"] = PRETTY.into();
        for event in [bash, fixture("posttooluse-mcp-text.json")] {
            let bytes = serde_json::to_vec(&event).unwrap();
            // The owner gate runs before any settings lookup or conflict scan.
            let env =
                |key: &str| -> Option<OsString> { panic!("env read before the owner gate: {key}") };
            // It also runs before any executor job: a job that ran would panic,
            // and the hook would report `job_panicked` instead of `Ok(None)`.
            fn never(_: &str) -> Compressed {
                panic!("executor job ran before the owner gate")
            }
            let gated = claude_hook(bytes.as_slice(), true, || false, env, &generous(), never);
            assert_eq!(gated, Ok(None));
            // Without the flag the gate is never consulted.
            let gate = || panic!("owner read without --minify-json");
            assert_eq!(
                claude_hook(bytes.as_slice(), false, gate, |_| None, &generous(), never),
                Ok(None)
            );
        }
    }

    #[test]
    fn conflicting_hook_blocks_the_replacement() {
        let dir = temp_dir("conflict");
        let settings = json!({"hooks": {"PostToolUse": [
            {"matcher": "Bash", "hooks": [{"type": "command", "command": "jq ."}]}
        ]}});
        std::fs::write(dir.join("settings.json"), settings.to_string()).unwrap();
        let mut event = fixture("posttooluse-bash.json");
        event["tool_response"]["stdout"] = PRETTY.into();
        let bytes = serde_json::to_vec(&event).unwrap();
        let message = hook_in(&dir, &bytes, true).unwrap_err();
        assert!(message.contains("settings.json"), "{message}");
        assert!(message.contains("unchanged"), "{message}");
        // No replacement, no scan: an unchanged result stays silent.
        assert_eq!(
            hook_in(
                &dir,
                &serde_json::to_vec(&fixture("posttooluse-bash.json")).unwrap(),
                true
            ),
            Ok(None)
        );
    }

    /// A pretty Bash event with `cwd` set, and a settings file with a matching hook.
    fn conflict_setup(name: &str) -> (std::path::PathBuf, Vec<u8>) {
        let dir = temp_dir(name);
        let settings = json!({"hooks": {"PostToolUse": [
            {"matcher": "Bash", "hooks": [{"type": "command", "command": "jq ."}]}
        ]}});
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join(".claude/settings.json"), settings.to_string()).unwrap();
        let mut event = fixture("posttooluse-bash.json");
        event["tool_response"]["stdout"] = PRETTY.into();
        event["cwd"] = dir.to_str().unwrap().into();
        (dir, serde_json::to_vec(&event).unwrap())
    }

    #[test]
    fn home_and_cwd_fallbacks_are_scanned() {
        let (home, _) = conflict_setup("home");
        let (cwd, bytes) = conflict_setup("cwd");
        let missing = temp_dir("fallback-none");
        // No CLAUDE_CONFIG_DIR: user settings come from $HOME/.claude. The project dir
        // points away from the event cwd, so only the HOME file can match.
        let message = claude(bytes.as_slice(), true, |key| match key {
            "HOME" => Some(home.as_os_str().to_owned()),
            "CLAUDE_PROJECT_DIR" => Some(missing.as_os_str().to_owned()),
            _ => None,
        })
        .unwrap_err();
        assert!(message.contains(home.to_str().unwrap()), "{message}");
        // No CLAUDE_PROJECT_DIR: the project settings come from the event cwd.
        let message = claude(bytes.as_slice(), true, |key| {
            (key == "CLAUDE_CONFIG_DIR").then(|| missing.as_os_str().to_owned())
        })
        .unwrap_err();
        assert!(message.contains(cwd.to_str().unwrap()), "{message}");
    }

    #[test]
    fn empty_env_values_count_as_unset() {
        let (home, bytes) = conflict_setup("empty-env");
        let missing = temp_dir("empty-env-none");
        // Empty CLAUDE_CONFIG_DIR falls through to HOME; empty CLAUDE_PROJECT_DIR to cwd.
        for (empty, other) in [
            ("CLAUDE_CONFIG_DIR", "CLAUDE_PROJECT_DIR"),
            ("CLAUDE_PROJECT_DIR", "CLAUDE_CONFIG_DIR"),
        ] {
            let result = claude(bytes.as_slice(), true, |key| match key {
                "HOME" => Some(home.as_os_str().to_owned()),
                k if k == empty => Some(OsString::new()),
                k if k == other => Some(missing.as_os_str().to_owned()),
                _ => None,
            });
            assert!(result.is_err(), "{empty}");
        }
        // Empty everywhere and no cwd: nothing to scan, so the replacement goes out.
        let mut event: Value = serde_json::from_slice(&bytes).unwrap();
        event.as_object_mut().unwrap().remove("cwd");
        let bytes = serde_json::to_vec(&event).unwrap();
        let output = claude(bytes.as_slice(), true, |_| Some(OsString::new()));
        assert!(matches!(output, Ok(Some(_))), "{output:?}");
    }

    /// Minifies, but far slower than any budget the tests below use for it.
    fn slow_on_b(text: &str) -> Compressed {
        if text.contains("\"b\"") {
            std::thread::sleep(std::time::Duration::from_secs(10));
        }
        minify(text)
    }

    fn pretty_bash() -> Vec<u8> {
        let mut event = fixture("posttooluse-bash.json");
        event["tool_response"]["stdout"] = "{\n  \"b\": 1\n}".into();
        serde_json::to_vec(&event).unwrap()
    }

    #[test]
    fn expired_job_leaves_output_unchanged() {
        let exec = BoundedExecutor::new(1, std::time::Duration::from_millis(1)).unwrap();
        let result = claude_hook(
            pretty_bash().as_slice(),
            true,
            || true,
            |_| None,
            &exec,
            slow_on_b,
        );
        assert_eq!(
            result,
            Err("compression budget_exceeded; tool output unchanged".to_string())
        );
    }

    #[test]
    fn one_expired_mcp_block_leaves_every_block_unchanged() {
        let mut event = fixture("posttooluse-mcp-text.json");
        event["tool_response"] = json!([
            {"type": "text", "text": PRETTY},
            {"type": "text", "text": "{\n  \"b\": 1\n}"},
        ]);
        let bytes = serde_json::to_vec(&event).unwrap();
        let exec = BoundedExecutor::new(1, std::time::Duration::from_millis(200)).unwrap();
        let result = claude_hook(bytes.as_slice(), true, || true, |_| None, &exec, slow_on_b);
        assert_eq!(
            result,
            Err("compression budget_exceeded; tool output unchanged".to_string())
        );
    }

    #[test]
    fn panicked_job_leaves_output_unchanged() {
        fn panic_on_b(text: &str) -> Compressed {
            assert!(!text.contains("\"b\""), "injected panic");
            minify(text)
        }
        let result = claude_hook(
            pretty_bash().as_slice(),
            true,
            || true,
            |_| None,
            &generous(),
            panic_on_b,
        );
        assert_eq!(
            result,
            Err("compression job_panicked; tool output unchanged".to_string())
        );
    }

    #[test]
    fn busy_executor_leaves_output_unchanged() {
        let exec = BoundedExecutor::new(1, std::time::Duration::from_secs(60)).unwrap();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let holder = {
            let exec = exec.clone();
            std::thread::spawn(move || exec.run(move || wait.recv()))
        };
        while exec.in_flight() == 0 {
            std::thread::yield_now();
        }
        let result = claude_hook(
            pretty_bash().as_slice(),
            true,
            || true,
            |_| None,
            &exec,
            minify,
        );
        assert_eq!(
            result,
            Err("compression busy; tool output unchanged".to_string())
        );
        drop(release);
        assert!(holder.join().unwrap().is_ok());
        // With the permit free again, the same executor minifies.
        let output = claude_hook(
            pretty_bash().as_slice(),
            true,
            || true,
            |_| Some("/nonexistent-deslag".into()),
            &exec,
            minify,
        );
        assert!(matches!(output, Ok(Some(_))), "{output:?}");
    }
}
