//! Finds other Claude Code `PostToolUse` hooks that may also replace a tool's output.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Settings and plugin hooks files larger than this are ignored.
const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;

/// True only for a whole command that runs deslag's hook: `<deslag> hook claude`
/// with an optional `--minify-json`, or the plugin wrapper `<deslag-hook.sh>` with
/// no arguments. The program may be a bare name or a path, optionally in double
/// quotes. Anything else, including a command that only mentions deslag, is foreign.
fn is_own_command(command: &str) -> bool {
    let command = command.trim();
    let (program, rest) = match command.strip_prefix('"') {
        Some(quoted) => match quoted.split_once('"') {
            Some(split) => split,
            None => return false,
        },
        None => command
            .split_once(char::is_whitespace)
            .unwrap_or((command, "")),
    };
    let name = program.rsplit('/').next().unwrap_or(program);
    let args: Vec<&str> = rest.split_whitespace().collect();
    match name {
        "deslag" => matches!(
            args[..],
            ["hook", "claude"] | ["hook", "claude", "--minify-json"]
        ),
        "deslag-hook.sh" => args.is_empty(),
        _ => false,
    }
}

/// Tools whose results deslag compresses. `mcp__*` stands for every MCP tool.
pub const ROUTED_TOOLS: [&str; 2] = ["Bash", "mcp__*"];

/// A settings or plugin hooks file with a foreign `PostToolUse` hook that may
/// match one of `ROUTED_TOOLS`.
#[derive(Debug)]
pub struct Conflict {
    pub path: PathBuf,
    /// The plugin key for a plugin hooks file; `None` for a settings file.
    pub plugin: Option<String>,
    pub tools: Vec<&'static str>,
}

/// Returns a diagnostic naming the first settings or plugin hooks file with a
/// `PostToolUse` hook, other than deslag's own, whose matcher may match `tool`.
/// Missing, unreadable and malformed files never report a conflict. The hooks
/// file of a plugin listed in `<state_dir>/ignore-hooks` is not scanned.
pub fn find(
    tool: &str,
    config_dir: Option<&Path>,
    project_dir: Option<&Path>,
    state_dir: Option<&Path>,
) -> Option<String> {
    let files = sources(config_dir, project_dir, state_dir);
    let (path, _, _) = files
        .iter()
        .find(|(_, _, file)| has_matching_hook(file, tool))?;
    Some(format!(
        "PostToolUse hook in {} also matches {tool}; leaving the output unchanged",
        path.display()
    ))
}

/// Every file `find` would report for a routed tool, in scan order, with the
/// routed tools it matches.
// ponytail: a literal MCP matcher such as `mcp__github__get_issue` does not match
// `mcp__*` here, so it is not listed. Match tool name prefixes when that hides a real conflict.
pub fn all(
    config_dir: Option<&Path>,
    project_dir: Option<&Path>,
    state_dir: Option<&Path>,
) -> Vec<Conflict> {
    sources(config_dir, project_dir, state_dir)
        .into_iter()
        .filter_map(|(path, plugin, file)| {
            let tools: Vec<&'static str> = ROUTED_TOOLS
                .into_iter()
                .filter(|tool| has_matching_hook(&file, tool))
                .collect();
            (!tools.is_empty()).then_some(Conflict {
                path,
                plugin,
                tools,
            })
        })
        .collect()
}

/// The settings files, then the hooks files of enabled plugins not listed in
/// `<state_dir>/ignore-hooks`, each with its plugin key and parsed JSON. Each
/// file appears once, also when the project `.claude` dir is the config dir or
/// several settings files enable the same plugin.
fn sources(
    config_dir: Option<&Path>,
    project_dir: Option<&Path>,
    state_dir: Option<&Path>,
) -> Vec<(PathBuf, Option<String>, Value)> {
    let ignored = state_dir.map(ignored_plugins).unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    let mut first = |path: &Path| {
        seen.insert(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
    };
    let mut paths = Vec::new();
    if let Some(dir) = config_dir {
        paths.push(dir.join("settings.json"));
    }
    if let Some(dir) = project_dir {
        paths.push(dir.join(".claude").join("settings.json"));
        paths.push(dir.join(".claude").join("settings.local.json"));
    }
    let mut files: Vec<(PathBuf, Option<String>, Value)> = paths
        .into_iter()
        .filter(|path| first(path))
        .filter_map(|path| read_json(&path).map(|value| (path, None, value)))
        .collect();
    if let Some(installed) =
        config_dir.and_then(|dir| read_json(&dir.join("plugins").join("installed_plugins.json")))
    {
        let enabled: Vec<String> = files
            .iter()
            .filter_map(|(_, _, settings)| settings.get("enabledPlugins")?.as_object())
            .flatten()
            .filter(|(_, on)| on.as_bool() == Some(true))
            .map(|(key, _)| key.clone())
            .filter(|key| !ignored.contains(key))
            .collect();
        for key in enabled {
            let installs = installed["plugins"][&key].as_array().into_iter().flatten();
            for path in installs.filter_map(|install| install["installPath"].as_str()) {
                let path = Path::new(path).join("hooks").join("hooks.json");
                if !first(&path) {
                    continue;
                }
                if let Some(hooks) = read_json(&path) {
                    files.push((path, Some(key.clone()), hooks));
                }
            }
        }
    }
    files
}

/// Plugin keys in `<state_dir>/ignore-hooks`, one per line. Blank lines and `#`
/// comments are skipped. A missing or unreadable file is an empty list.
fn ignored_plugins(state_dir: &Path) -> Vec<String> {
    let Some(buf) = read_bounded(&state_dir.join("ignore-hooks")) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&buf)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&read_bounded(path)?).ok()
}

/// Reads a regular file of at most `MAX_SETTINGS_BYTES`. The path is checked
/// before opening so a FIFO or device never blocks the hook, and the open file
/// is checked again in case the path changed in between.
fn read_bounded(path: &Path) -> Option<Vec<u8>> {
    if !std::fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut buf = Vec::new();
    file.take(MAX_SETTINGS_BYTES + 1)
        .read_to_end(&mut buf)
        .ok()?;
    (buf.len() as u64 <= MAX_SETTINGS_BYTES).then_some(buf)
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
        .any(|hook| !hook["command"].as_str().is_some_and(is_own_command))
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
        find("Bash", Some(&dir), None, None)
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
        assert!(find("Bash", Some(&dir), None, None).is_some());
    }

    #[test]
    fn own_hook_and_other_events_are_ignored() {
        for (command, conflict) in [
            ("\"${CLAUDE_PLUGIN_ROOT}/bin/deslag\" hook claude", false),
            ("/x/deslag hook claude --minify-json", false),
            (PLUGIN_WRAPPER, false),
            ("~/github/deslag/scripts/fmt.sh", true),
            ("jq . # deslag-safe", true),
            ("deslag hook claude", false),
            ("\"/a b/deslag\" hook claude --minify-json", false),
            ("/x/deslag-hook.sh", false),
            ("jq '.note = \"deslag hook claude\"'", true),
            ("/x/other-deslag-hook.sh", true),
            ("/x/notdeslag hook claude", true),
            ("/x/deslag hook claude; jq .", true),
            ("/x/deslag hook claude --other", true),
            ("/x/deslag-hook.sh && jq .", true),
            ("\"/x/deslag hook claude", true),
        ] {
            let found = user_settings("own", "Bash", command).is_some();
            assert_eq!(found, conflict, "{command:?}");
        }
        let dir = temp_dir("pre");
        let settings = serde_json::json!({"hooks": {"PreToolUse": [
            {"matcher": "Bash", "hooks": [{"type": "command", "command": "jq ."}]}
        ]}});
        write(&dir.join("settings.json"), &settings);
        assert_eq!(find("Bash", Some(&dir), None, None), None);
    }

    #[test]
    fn project_and_local_settings_are_scanned() {
        for file in ["settings.json", "settings.local.json"] {
            let project = temp_dir(&format!("project-{file}"));
            let path = project.join(".claude").join(file);
            write(&path, &hooks("Bash", "jq ."));
            let message = find("Bash", None, Some(&project), None).unwrap();
            assert!(message.contains(&path.display().to_string()), "{message}");
        }
    }

    const PLUGIN_WRAPPER: &str = "\"${CLAUDE_PLUGIN_ROOT}/hooks/deslag-hook.sh\"";

    fn plugin_setup(name: &str, key: &str, enabled: bool, command: &str) -> PathBuf {
        let dir = temp_dir(name);
        let install = dir.join("plugins/cache/other");
        write(&install.join("hooks/hooks.json"), &hooks("Bash", command));
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
        let dir = plugin_setup("plugin-on", "other@market", true, "jq .");
        let message = find("Bash", Some(&dir), None, None).unwrap();
        assert!(message.contains("hooks.json"), "{message}");
    }

    #[test]
    fn disabled_and_own_plugins_are_ignored() {
        let dir = plugin_setup("plugin-off", "other@market", false, "jq .");
        assert_eq!(find("Bash", Some(&dir), None, None), None);
        let dir = plugin_setup("plugin-own", "deslag@deslag", true, PLUGIN_WRAPPER);
        assert_eq!(find("Bash", Some(&dir), None, None), None);
        // The plugin key alone does not mark a hook as deslag's own.
        let dir = plugin_setup("plugin-named", "deslag@deslag", true, "jq .");
        assert!(find("Bash", Some(&dir), None, None).is_some());
    }

    #[test]
    fn listed_plugins_are_skipped() {
        let dir = plugin_setup("ignore-listed", "other@market", true, "jq .");
        fs::write(
            dir.join("ignore-hooks"),
            "# observers\n\n  other@market  \n",
        )
        .unwrap();
        assert_eq!(find("Bash", Some(&dir), None, Some(&dir)), None);
        fs::write(dir.join("ignore-hooks"), "else@market\n").unwrap();
        assert!(find("Bash", Some(&dir), None, Some(&dir)).is_some());
    }

    #[test]
    fn settings_hooks_are_never_skipped() {
        let dir = plugin_setup("ignore-settings", "other@market", true, "jq .");
        let mut settings = hooks("Bash", "jq .");
        settings["enabledPlugins"] = serde_json::json!({"other@market": true});
        write(&dir.join("settings.json"), &settings);
        fs::write(dir.join("ignore-hooks"), "other@market\n").unwrap();
        let message = find("Bash", Some(&dir), None, Some(&dir)).unwrap();
        assert!(message.contains("settings.json"), "{message}");
    }

    #[test]
    fn missing_empty_and_unreadable_ignore_lists_change_nothing() {
        let dir = plugin_setup("ignore-none", "other@market", true, "jq .");
        assert!(find("Bash", Some(&dir), None, Some(&dir)).is_some());
        for text in ["", "# only a comment\n\n"] {
            fs::write(dir.join("ignore-hooks"), text).unwrap();
            assert!(find("Bash", Some(&dir), None, Some(&dir)).is_some());
        }
        fs::remove_file(dir.join("ignore-hooks")).unwrap();
        fs::create_dir(dir.join("ignore-hooks")).unwrap();
        assert!(ignored_plugins(&dir).is_empty());
        assert!(find("Bash", Some(&dir), None, Some(&dir)).is_some());
    }

    #[test]
    fn oversize_and_non_regular_files_are_ignored() {
        let dir = temp_dir("oversize");
        let settings = hooks("Bash", "jq .").to_string();
        let padding = MAX_SETTINGS_BYTES as usize - settings.len();
        fs::write(
            dir.join("settings.json"),
            settings.clone() + &" ".repeat(padding),
        )
        .unwrap();
        assert!(find("Bash", Some(&dir), None, None).is_some());
        fs::write(
            dir.join("settings.json"),
            settings + &" ".repeat(padding + 1),
        )
        .unwrap();
        assert_eq!(find("Bash", Some(&dir), None, None), None);

        let dir = temp_dir("directory");
        fs::create_dir_all(dir.join("settings.json")).unwrap();
        assert_eq!(read_json(&dir.join("settings.json")), None);
        assert_eq!(find("Bash", Some(&dir), None, None), None);
    }

    #[test]
    fn missing_and_malformed_files_are_ignored() {
        let dir = temp_dir("malformed");
        assert_eq!(find("Bash", Some(&dir), Some(&dir), None), None);
        fs::write(dir.join("settings.json"), "{not json").unwrap();
        fs::create_dir_all(dir.join(".claude")).unwrap();
        fs::write(dir.join(".claude/settings.json"), r#"{"hooks":[1]}"#).unwrap();
        assert_eq!(find("Bash", Some(&dir), Some(&dir), None), None);
    }

    #[test]
    fn all_lists_each_source_with_its_plugin_and_routed_tools() {
        let dir = plugin_setup("all", "other@market", true, "jq .");
        let mut settings = hooks("mcp__.*", "jq .");
        settings["enabledPlugins"] = serde_json::json!({"other@market": true});
        write(&dir.join("settings.json"), &settings);
        let found = all(Some(&dir), None, Some(&dir));
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].path, dir.join("settings.json"));
        assert_eq!(found[0].plugin, None);
        // A regex matcher counts as matching every tool.
        assert_eq!(found[0].tools, ["Bash", "mcp__*"]);
        assert!(found[1].path.ends_with("hooks/hooks.json"));
        assert_eq!(found[1].plugin.as_deref(), Some("other@market"));
        assert_eq!(found[1].tools, ["Bash"]);
        fs::write(dir.join("ignore-hooks"), "other@market\n").unwrap();
        let found = all(Some(&dir), None, Some(&dir));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].plugin, None);
    }

    #[test]
    fn all_lists_a_file_once_when_the_project_claude_dir_is_the_config_dir() {
        let home = temp_dir("home-project");
        let config = home.join(".claude");
        let install = home.join("plugins/cache/other");
        write(&install.join("hooks/hooks.json"), &hooks("Bash", "jq ."));
        write(
            &config.join("plugins/installed_plugins.json"),
            &serde_json::json!({"version": 2, "plugins": {
                "other@market": [{"installPath": install, "version": "1.0.0"}]
            }}),
        );
        let mut settings = hooks("Bash", "jq .");
        settings["enabledPlugins"] = serde_json::json!({"other@market": true});
        write(&config.join("settings.json"), &settings);
        let found = all(Some(&config), Some(&home), None);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].plugin, None);
        assert_eq!(found[1].plugin.as_deref(), Some("other@market"));
    }

    #[test]
    fn all_lists_a_plugin_once_when_two_settings_files_enable_it() {
        let dir = plugin_setup("two-enable", "other@market", true, "jq .");
        let project = temp_dir("two-enable-project");
        write(
            &project.join(".claude/settings.json"),
            &serde_json::json!({"enabledPlugins": {"other@market": true}}),
        );
        let found = all(Some(&dir), Some(&project), None);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].plugin.as_deref(), Some("other@market"));
    }
}
