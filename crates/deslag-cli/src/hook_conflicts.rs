//! Finds other Claude Code `PostToolUse` hooks that may also replace a tool's output.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Returns a diagnostic naming the first settings or plugin hooks file with a
/// `PostToolUse` hook, other than deslag's own, whose matcher may match `tool`.
/// Missing, unreadable and malformed files never report a conflict.
pub fn find(tool: &str, config_dir: Option<&Path>, project_dir: Option<&Path>) -> Option<String> {
    let mut paths = Vec::new();
    if let Some(dir) = config_dir {
        paths.push(dir.join("settings.json"));
    }
    if let Some(dir) = project_dir {
        paths.push(dir.join(".claude").join("settings.json"));
        paths.push(dir.join(".claude").join("settings.local.json"));
    }
    let mut files: Vec<(PathBuf, Value)> = paths
        .into_iter()
        .filter_map(|path| read_json(&path).map(|value| (path, value)))
        .collect();
    if let Some(installed) =
        config_dir.and_then(|dir| read_json(&dir.join("plugins").join("installed_plugins.json")))
    {
        let enabled: Vec<String> = files
            .iter()
            .filter_map(|(_, settings)| settings.get("enabledPlugins")?.as_object())
            .flatten()
            .filter(|(key, on)| on.as_bool() == Some(true) && !key.starts_with("deslag@"))
            .map(|(key, _)| key.clone())
            .collect();
        for key in enabled {
            let installs = installed["plugins"][&key].as_array().into_iter().flatten();
            for path in installs.filter_map(|install| install["installPath"].as_str()) {
                let path = Path::new(path).join("hooks").join("hooks.json");
                if let Some(hooks) = read_json(&path) {
                    files.push((path, hooks));
                }
            }
        }
    }
    let (path, _) = files
        .iter()
        .find(|(_, file)| has_matching_hook(file, tool))?;
    Some(format!(
        "PostToolUse hook in {} also matches {tool}; leaving the output unchanged",
        path.display()
    ))
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// A matcher group counts when its matcher may match `tool` and it holds any
/// hook whose command is not deslag's own.
fn has_matching_hook(file: &Value, tool: &str) -> bool {
    let groups = file["hooks"]["PostToolUse"]
        .as_array()
        .into_iter()
        .flatten();
    groups
        .filter(|group| matcher_matches(group.get("matcher").and_then(Value::as_str), tool))
        .flat_map(|group| group["hooks"].as_array().into_iter().flatten())
        .any(|hook| {
            !hook["command"]
                .as_str()
                .is_some_and(|c| c.contains("deslag"))
        })
}

fn matcher_matches(matcher: Option<&str>, tool: &str) -> bool {
    match matcher {
        None | Some("" | "*") => true,
        Some(m)
            if m.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '|') =>
        {
            m.split('|').any(|alternative| alternative == tool)
        }
        // ponytail: regex matchers are not evaluated and count as matching, which can
        // report false conflicts. Evaluate them when that blocks real setups.
        Some(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("deslag-conflicts-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, value: &Value) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value.to_string()).unwrap();
    }

    fn hooks(matcher: &str, command: &str) -> Value {
        serde_json::json!({"hooks": {"PostToolUse": [
            {"matcher": matcher, "hooks": [{"type": "command", "command": command}]}
        ]}})
    }

    fn user_settings(name: &str, matcher: &str, command: &str) -> Option<String> {
        let dir = temp_dir(name);
        write(&dir.join("settings.json"), &hooks(matcher, command));
        find("Bash", Some(&dir), None)
    }

    #[test]
    fn matching_user_hook_is_reported() {
        let message = user_settings("user", "Bash", "jq .").unwrap();
        assert!(message.contains("settings.json"), "{message}");
        assert!(message.contains("Bash"), "{message}");
    }

    #[test]
    fn matcher_semantics() {
        for (matcher, conflict) in [
            ("Bash", true),
            ("", true),
            ("*", true),
            ("Edit|Bash", true),
            ("mcp__.*", true),
            ("Read", false),
            ("Bas", false),
            ("Edit|Read", false),
        ] {
            let found = user_settings("matcher", matcher, "jq .").is_some();
            assert_eq!(found, conflict, "{matcher:?}");
        }
        let dir = temp_dir("no-matcher");
        let settings = serde_json::json!({"hooks": {"PostToolUse": [
            {"hooks": [{"type": "command", "command": "jq ."}]}
        ]}});
        write(&dir.join("settings.json"), &settings);
        assert!(find("Bash", Some(&dir), None).is_some());
    }

    #[test]
    fn own_hook_and_other_events_are_ignored() {
        assert_eq!(
            user_settings(
                "own",
                "Bash",
                "\"${CLAUDE_PLUGIN_ROOT}/bin/deslag\" hook claude"
            ),
            None
        );
        let dir = temp_dir("pre");
        let settings = serde_json::json!({"hooks": {"PreToolUse": [
            {"matcher": "Bash", "hooks": [{"type": "command", "command": "jq ."}]}
        ]}});
        write(&dir.join("settings.json"), &settings);
        assert_eq!(find("Bash", Some(&dir), None), None);
    }

    #[test]
    fn project_and_local_settings_are_scanned() {
        for file in ["settings.json", "settings.local.json"] {
            let project = temp_dir(&format!("project-{file}"));
            let path = project.join(".claude").join(file);
            write(&path, &hooks("Bash", "jq ."));
            let message = find("Bash", None, Some(&project)).unwrap();
            assert!(message.contains(&path.display().to_string()), "{message}");
        }
    }

    fn plugin_setup(name: &str, key: &str, enabled: bool) -> PathBuf {
        let dir = temp_dir(name);
        let install = dir.join("plugins/cache/other");
        write(&install.join("hooks/hooks.json"), &hooks("Bash", "jq ."));
        write(
            &dir.join("plugins/installed_plugins.json"),
            &serde_json::json!({"version": 2, "plugins": {
                key: [{"installPath": install, "version": "1.0.0"}]
            }}),
        );
        write(
            &dir.join("settings.json"),
            &serde_json::json!({"enabledPlugins": {key: enabled}}),
        );
        dir
    }

    #[test]
    fn enabled_plugin_hooks_are_reported() {
        let dir = plugin_setup("plugin-on", "other@market", true);
        let message = find("Bash", Some(&dir), None).unwrap();
        assert!(message.contains("hooks.json"), "{message}");
    }

    #[test]
    fn disabled_and_own_plugins_are_ignored() {
        let dir = plugin_setup("plugin-off", "other@market", false);
        assert_eq!(find("Bash", Some(&dir), None), None);
        let dir = plugin_setup("plugin-own", "deslag@deslag", true);
        assert_eq!(find("Bash", Some(&dir), None), None);
    }

    #[test]
    fn missing_and_malformed_files_are_ignored() {
        let dir = temp_dir("malformed");
        assert_eq!(find("Bash", Some(&dir), Some(&dir)), None);
        fs::write(dir.join("settings.json"), "{not json").unwrap();
        fs::create_dir_all(dir.join(".claude")).unwrap();
        fs::write(dir.join(".claude/settings.json"), r#"{"hooks":[1]}"#).unwrap();
        assert_eq!(find("Bash", Some(&dir), Some(&dir)), None);
    }
}
