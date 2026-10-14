//! `deslag codec --protocol 1`: the experimental stdin/stdout bridge a Claude
//! Code mod calls through `$.process.run`. One request envelope in, one
//! response envelope out. It reuses the hook's compression job and executor.
//!
//! Transport rules: stdout carries exactly one JSON line and nothing else;
//! diagnostics go to stderr. A malformed request exits nonzero with no stdout,
//! so the caller keeps the original content. Texts cross as JSON strings, so
//! the numbers inside a payload are never parsed or rounded.

use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

use deslag_core::BoundedExecutor;
use serde_json::{Value, json};

use crate::Job;

/// The only `owner` file value under which the codec emits replacements.
/// The hook and the proxy read any value other than `hook`/`proxy` as `none`,
/// so this value makes both stand down without a change to either.
pub const MOD_OWNER: &str = "mod";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Off,
    Observe,
    Safe,
}

/// Reads one request (at most `max_bytes`) and returns the response line.
/// `owner_is_mod` is read only for a `safe` request.
pub fn run(
    mut input: impl Read,
    max_bytes: u64,
    owner_is_mod: impl FnOnce() -> bool,
    exec: &BoundedExecutor,
    job: Job,
) -> Result<String, String> {
    let mut buf = Vec::new();
    input
        .by_ref()
        .take(max_bytes + 1)
        .read_to_end(&mut buf)
        .map_err(|err| format!("cannot read codec request: {err}"))?;
    if buf.len() as u64 > max_bytes {
        let _ = std::io::copy(&mut input, &mut std::io::sink());
        return Err(format!("codec request exceeds {max_bytes} bytes"));
    }
    let request: Value =
        serde_json::from_slice(&buf).map_err(|err| format!("malformed codec request: {err}"))?;
    if request["schema_version"] != 1 {
        return Err("unsupported schema_version".into());
    }
    let mode = match request["mode"].as_str() {
        Some("off") => Mode::Off,
        Some("observe") => Mode::Observe,
        Some("safe") => Mode::Safe,
        _ => return Err("mode must be off, observe or safe".into()),
    };
    if !matches!(request["tool_category"].as_str(), Some("bash" | "mcp")) {
        return Err("tool_category must be bash or mcp".into());
    }
    // The caller's policy label is echoed so it can reject a mismatched reply.
    let policy = request["policy_version"]
        .as_str()
        .ok_or("policy_version must be a string")?;
    let items = request["items"]
        .as_array()
        .ok_or("items must be an array")?;
    let mut slots = Vec::with_capacity(items.len());
    let mut seen = HashSet::with_capacity(items.len());
    for item in items {
        let slot = item["slot"]
            .as_u64()
            .ok_or("slot must be an unsigned integer")?;
        let text = item["text"].as_str().ok_or("text must be a string")?;
        if !seen.insert(slot) {
            return Err(format!("duplicate slot {slot}"));
        }
        slots.push((slot, text));
    }
    // A safe request without mod ownership changes nothing. The owner file is
    // per state directory, so it applies to every session that shares it.
    let owner_gate = mode == Mode::Safe && !owner_is_mod();
    let out: Vec<Value> = slots
        .into_iter()
        .map(|(slot, text)| item(slot, text, mode, owner_gate, exec, job))
        .collect();
    Ok(
        json!({"schema_version": 1, "policy_version": policy, "complete": true, "items": out})
            .to_string(),
    )
}

fn item(
    slot: u64,
    text: &str,
    mode: Mode,
    owner_gate: bool,
    exec: &BoundedExecutor,
    job: Job,
) -> Value {
    let skipped = |reason: &str| {
        json!({"slot": slot, "outcome": "skipped", "reason": reason,
               "replacement": null, "replacement_utf8_bytes": null, "report": null})
    };
    if mode == Mode::Off {
        return skipped("mode_off");
    }
    if owner_gate {
        return skipped("not_owner");
    }
    let owned = text.to_owned();
    let (changed, report) = match exec.run(move || job(&owned)) {
        Ok(done) => done,
        Err(rejected) => {
            return json!({"slot": slot, "outcome": "failed", "reason": rejected.as_str(),
                          "replacement": null, "replacement_utf8_bytes": null, "report": null});
        }
    };
    let mut report: Value = serde_json::from_str(&report.to_json()).unwrap_or(Value::Null);
    let (outcome, replacement) = match (changed, mode) {
        (Some(candidate), Mode::Safe) => ("changed", Some(candidate)),
        (Some(_), _) => {
            // The job runs the safe context; observe emits the original.
            report["outcome"] = "would_change".into();
            report["emitted_bytes"] = report["original_bytes"].clone();
            report["emitted_tokens"] = report["original_tokens"].clone();
            ("would_change", None)
        }
        (None, _) => (
            match report["outcome"].as_str() {
                Some("failed") => "failed",
                _ => "skipped",
            },
            None,
        ),
    };
    if mode == Mode::Observe && report.is_object() {
        report["mode"] = "observe".into();
    }
    json!({
        "slot": slot,
        "outcome": outcome,
        "reason": report["skip_or_failure_reason"],
        "replacement_utf8_bytes": replacement.as_ref().map(String::len),
        "replacement": replacement,
        "report": report,
    })
}

/// True when the state directory's `owner` file names the mod.
// ponytail: second owner-file reader next to `deslag_proxy::session::read_owner`,
// which owns the format. Phase-mod-1 step 3 adds `Owner::Mod` and retires this parser.
pub fn owner_is_mod(dir: Option<&Path>) -> bool {
    dir.and_then(|dir| std::fs::read_to_string(dir.join("owner")).ok())
        .is_some_and(|owner| owner.trim_end() == MOD_OWNER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minify;

    const PRETTY: &str = "{\n  \"a\": [\n    1,\n    2\n  ]\n}";
    const COMPACT: &str = r#"{"a":[1,2]}"#;

    fn generous() -> BoundedExecutor {
        BoundedExecutor::new(4, std::time::Duration::from_secs(60)).unwrap()
    }

    fn request(mode: &str, texts: &[&str]) -> String {
        let items: Vec<Value> = texts
            .iter()
            .enumerate()
            .map(|(slot, text)| json!({"slot": slot, "text": text}))
            .collect();
        json!({"schema_version": 1, "policy_version": "spike-1", "mode": mode,
               "tool_category": "mcp", "items": items})
        .to_string()
    }

    fn call(input: &str, owner: bool) -> Result<Value, String> {
        run(input.as_bytes(), 1 << 20, || owner, &generous(), minify)
            .map(|line| serde_json::from_str(&line).unwrap())
    }

    #[test]
    fn safe_with_mod_owner_changes_every_minifiable_slot() {
        let out = call(&request("safe", &[PRETTY, "plain text", PRETTY]), true).unwrap();
        assert_eq!(out["complete"], true);
        assert_eq!(out["policy_version"], "spike-1");
        let items = out["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        for i in [0, 2] {
            assert_eq!(items[i]["slot"], i);
            assert_eq!(items[i]["outcome"], "changed");
            assert_eq!(items[i]["replacement"], COMPACT);
            assert_eq!(items[i]["replacement_utf8_bytes"], COMPACT.len());
            assert_eq!(items[i]["report"]["outcome"], "changed");
        }
        assert_eq!(items[1]["outcome"], "skipped");
        assert_eq!(items[1]["replacement"], Value::Null);
    }

    #[test]
    fn observe_reports_would_change_without_a_replacement() {
        let out = call(&request("observe", &[PRETTY, "plain text"]), false).unwrap();
        let item = &out["items"][0];
        assert_eq!(item["outcome"], "would_change");
        assert_eq!(item["replacement"], Value::Null);
        assert_eq!(item["report"]["outcome"], "would_change");
        assert_eq!(item["report"]["mode"], "observe");
        assert_eq!(item["report"]["emitted_bytes"], PRETTY.len());
        let skipped = &out["items"][1];
        assert_eq!(skipped["outcome"], "skipped");
        assert_eq!(skipped["report"]["mode"], "observe");
    }

    #[test]
    fn safe_without_mod_owner_changes_nothing() {
        let out = call(&request("safe", &[PRETTY]), false).unwrap();
        assert_eq!(out["items"][0]["outcome"], "skipped");
        assert_eq!(out["items"][0]["reason"], "not_owner");
    }

    #[test]
    fn off_and_observe_never_read_the_owner() {
        for mode in ["off", "observe"] {
            let input = request(mode, &[PRETTY]);
            let gate = || panic!("owner read in {mode}");
            let line = run(input.as_bytes(), 1 << 20, gate, &generous(), minify).unwrap();
            let out: Value = serde_json::from_str(&line).unwrap();
            assert_ne!(out["items"][0]["outcome"], "changed", "{line}");
            if mode == "off" {
                assert_eq!(out["items"][0]["outcome"], "skipped", "{line}");
                assert_eq!(out["items"][0]["reason"], "mode_off", "{line}");
            }
        }
    }

    #[test]
    fn number_lexemes_in_text_stay_byte_exact() {
        let text = "{\n \"big\": 123456789012345678901234567890,\n \"dec\": 0.100000000000000000000000000001\n}";
        let out = call(&request("safe", &[text]), true).unwrap();
        assert_eq!(
            out["items"][0]["replacement"],
            r#"{"big":123456789012345678901234567890,"dec":0.100000000000000000000000000001}"#
        );
    }

    #[test]
    fn utf8_byte_count_is_bytes_not_chars() {
        let out = call(&request("safe", &["{\n \"k\": \"äöü😀\"\n}"]), true).unwrap();
        let replacement = out["items"][0]["replacement"].as_str().unwrap();
        assert_eq!(out["items"][0]["replacement_utf8_bytes"], replacement.len());
        assert!(replacement.len() > replacement.chars().count());
    }

    #[test]
    fn malformed_requests_fail_without_output() {
        let base = json!({"schema_version": 1, "policy_version": "p", "mode": "safe",
                          "tool_category": "bash", "items": [{"slot": 0, "text": PRETTY}]});
        assert!(call(&base.to_string(), true).is_ok());
        let mut missing_policy = base.clone();
        missing_policy
            .as_object_mut()
            .unwrap()
            .remove("policy_version");
        let mut cases = vec!["not json".to_string(), missing_policy.to_string()];
        for (key, bad) in [
            ("policy_version", Value::Null),
            ("policy_version", json!(1)),
            ("policy_version", json!({})),
            ("policy_version", json!([])),
            ("schema_version", json!(2)),
            ("mode", json!("aggressive")),
            ("tool_category", json!("read")),
            ("items", json!({})),
            ("items", json!([{"slot": -1, "text": "x"}])),
            ("items", json!([{"slot": 0, "text": 1}])),
            (
                "items",
                json!([{"slot": 0, "text": "a"}, {"slot": 0, "text": "b"}]),
            ),
            (
                "items",
                json!([{"slot": 7, "text": "a"}, {"slot": 1, "text": "b"}, {"slot": 7, "text": "c"}]),
            ),
        ] {
            let mut req = base.clone();
            req[key] = bad;
            cases.push(req.to_string());
        }
        for case in cases {
            assert!(call(&case, true).is_err(), "{case}");
        }
    }

    #[test]
    fn accepted_envelopes_echo_policy_and_keep_slot_order() {
        let mut req = json!({"schema_version": 1, "policy_version": "", "mode": "safe",
                             "tool_category": "bash", "items": []});
        let out = call(&req.to_string(), true).unwrap();
        assert_eq!(out["policy_version"], "");
        assert_eq!(out["items"], json!([]));
        req["items"] =
            json!([{"slot": 9, "text": "a"}, {"slot": 0, "text": "b"}, {"slot": 4, "text": "c"}]);
        let out = call(&req.to_string(), true).unwrap();
        let slots: Vec<&Value> = out["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| &i["slot"])
            .collect();
        assert_eq!(slots, [&json!(9), &json!(0), &json!(4)]);
    }

    #[test]
    fn oversize_request_is_rejected() {
        let input = request("safe", &[&" ".repeat(64)]);
        let result = run(input.as_bytes(), 16, || true, &generous(), minify);
        assert!(result.unwrap_err().contains("exceeds"));
    }

    #[test]
    fn rejected_job_is_a_failed_slot() {
        fn boom(_: &str) -> crate::Compressed {
            panic!("injected")
        }
        let input = request("safe", &[PRETTY]);
        let line = run(input.as_bytes(), 1 << 20, || true, &generous(), boom).unwrap();
        let out: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(out["items"][0]["outcome"], "failed");
        assert_eq!(out["items"][0]["reason"], "job_panicked");
        assert_eq!(out["items"][0]["replacement"], Value::Null);
    }

    #[test]
    fn owner_file_must_name_the_mod() {
        let dir = std::env::temp_dir().join(format!("deslag-codec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!owner_is_mod(None));
        assert!(!owner_is_mod(Some(&dir.join("missing"))));
        for (value, expected) in [
            ("mod\n", true),
            ("hook", false),
            ("proxy", false),
            ("modx", false),
        ] {
            std::fs::write(dir.join("owner"), value).unwrap();
            assert_eq!(owner_is_mod(Some(&dir)), expected, "{value:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
