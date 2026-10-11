//! `deslag` command-line entry point.

use std::io::{self, Read, Write};
use std::process::ExitCode;

use serde_json::{Value, json};

const USAGE: &str = "usage: deslag hook claude [--minify-json]";

// Exit 2 from a Claude Code hook is blocking and feeds stderr to the model; usage errors avoid it.
const EX_USAGE: u8 = 64;

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
            match claude_hook(io::stdin().lock(), !flags.is_empty()) {
                // A failed write leaves the tool output unchanged, which is the safe default.
                Ok(Some(output)) => drop(writeln!(io::stdout().lock(), "{output}")),
                Ok(None) => {}
                Err(message) => eprintln!("deslag: {message}"),
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(EX_USAGE)
        }
    }
}

/// Validates a Claude Code PostToolUse event. With `minify_json`, returns the hook
/// output that replaces a Bash result whose stdout is minifiable JSON. `None`
/// leaves the tool output unchanged.
///
/// Errors are diagnostics only. The caller reports them on stderr and still exits 0,
/// so the hook never blocks the tool.
fn claude_hook(mut input: impl Read, minify_json: bool) -> Result<Option<Value>, String> {
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
    // Only Bash stdout is eligible. Read output stays byte-exact because Edit matches on it.
    if !minify_json || tool_name != "Bash" {
        return Ok(None);
    }
    let response = &event["tool_response"];
    let Some(minified) = response
        .get("stdout")
        .and_then(Value::as_str)
        .and_then(deslag_core::minify_json)
    else {
        return Ok(None);
    };
    let mut updated = response.clone();
    updated["stdout"] = Value::String(minified);
    Ok(replacement(&event, updated))
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
            assert_eq!(claude_hook(bytes.as_slice(), false), Ok(None));
        }
    }

    #[test]
    fn oversize_input_is_rejected() {
        let bytes = vec![b' '; MAX_HOOK_INPUT_BYTES as usize + 1];
        assert!(
            claude_hook(bytes.as_slice(), false)
                .unwrap_err()
                .contains("exceeds")
        );
    }

    #[test]
    fn input_at_the_cap_is_read() {
        let bytes = vec![b' '; MAX_HOOK_INPUT_BYTES as usize];
        assert!(
            claude_hook(bytes.as_slice(), false)
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
            assert!(claude_hook(input.as_bytes(), false).is_err(), "{input}");
        }
    }

    #[test]
    fn non_object_tool_responses_pass() {
        for input in [
            r#"{"hook_event_name":"PostToolUse","tool_name":"mcp__x__y","tool_response":"x"}"#,
            r#"{"hook_event_name":"PostToolUse","tool_name":"mcp__x__y","tool_response":[{"type":"text","text":"x"}]}"#,
        ] {
            assert_eq!(claude_hook(input.as_bytes(), true), Ok(None), "{input}");
        }
    }
}
