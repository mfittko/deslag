//! `deslag` command-line entry point.

use std::io::{self, Read};
use std::process::ExitCode;

use serde_json::{Value, json};

const USAGE: &str = "usage: deslag hook claude";

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
        ["hook", "claude"] => {
            if let Err(message) = claude_hook(io::stdin().lock()) {
                eprintln!("deslag: {message}");
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Validates a Claude Code PostToolUse event and leaves the tool output unchanged.
///
/// Errors are diagnostics only. The caller reports them on stderr and still exits 0,
/// so the hook never blocks the tool.
fn claude_hook(input: impl Read) -> Result<(), String> {
    let mut buf = Vec::new();
    input
        .take(MAX_HOOK_INPUT_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|err| format!("cannot read hook input: {err}"))?;
    if buf.len() as u64 > MAX_HOOK_INPUT_BYTES {
        return Err(format!(
            "hook input exceeds {MAX_HOOK_INPUT_BYTES} bytes; passing through"
        ));
    }
    let event: Value =
        serde_json::from_slice(&buf).map_err(|err| format!("malformed hook input: {err}"))?;
    match event.get("hook_event_name").and_then(Value::as_str) {
        Some("PostToolUse") => {}
        Some(other) => return Err(format!("unsupported hook event: {other}")),
        None => return Err("hook input has no hook_event_name".to_string()),
    }
    if event.get("tool_name").and_then(Value::as_str).is_none() {
        return Err("hook input has no tool_name".to_string());
    }
    if !event.get("tool_response").is_some_and(Value::is_object) {
        return Err("hook input has no object tool_response".to_string());
    }
    Ok(())
}

/// Builds the PostToolUse output that replaces the tool result with `updated`.
///
/// Claude Code drops an `updatedToolOutput` that does not match the tool's output
/// schema, so this returns `None` unless `updated` has the same shape as the
/// original `tool_response`.
#[cfg_attr(not(test), allow(dead_code))]
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

/// Same JSON type; objects also need the same key set with same-shaped values.
fn same_shape(original: &Value, updated: &Value) -> bool {
    match (original, updated) {
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| same_shape(value, other)))
        }
        (Value::Null, Value::Null)
        | (Value::Bool(_), Value::Bool(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::String(_), Value::String(_))
        | (Value::Array(_), Value::Array(_)) => true,
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
    fn valid_events_pass() {
        for name in ["posttooluse-bash.json", "posttooluse-read.json"] {
            let bytes = serde_json::to_vec(&fixture(name)).unwrap();
            assert_eq!(claude_hook(bytes.as_slice()), Ok(()));
        }
    }

    #[test]
    fn oversize_input_is_rejected() {
        let bytes = vec![b' '; MAX_HOOK_INPUT_BYTES as usize + 1];
        assert!(
            claude_hook(bytes.as_slice())
                .unwrap_err()
                .contains("exceeds")
        );
    }

    #[test]
    fn wrong_event_and_missing_fields_are_rejected() {
        for input in [
            r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_response":{}}"#,
            r#"{"tool_name":"Bash","tool_response":{}}"#,
            r#"{"hook_event_name":"PostToolUse","tool_response":{}}"#,
            r#"{"hook_event_name":"PostToolUse","tool_name":"Bash","tool_response":"x"}"#,
        ] {
            assert!(claude_hook(input.as_bytes()).is_err(), "{input}");
        }
    }
}
