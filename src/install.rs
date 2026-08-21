use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::paths;

/// Embedded at compile time so `install` never depends on the source
/// checkout being present at runtime (unlike the bash tool's install.sh,
/// which copies config/default.conf relative to its own script path).
const DEFAULT_CONFIG: &str = include_str!("../config/default.toml");

struct HookSpec {
    /// Claude Code's hooks.json key, e.g. "Notification".
    event_key: &'static str,
    /// Notification's own matcher, if any (written into the hook entry).
    matcher: Option<&'static str>,
    /// Our own `--event` value.
    our_event: &'static str,
    /// Our own `--matcher` value, if any.
    our_matcher: Option<&'static str>,
}

const HOOKS: &[HookSpec] = &[
    HookSpec {
        event_key: "UserPromptSubmit",
        matcher: None,
        our_event: "user-prompt-submit",
        our_matcher: None,
    },
    HookSpec {
        event_key: "Notification",
        matcher: Some("permission_prompt"),
        our_event: "notification",
        our_matcher: Some("permission_prompt"),
    },
    HookSpec {
        event_key: "Notification",
        matcher: Some("idle_prompt"),
        our_event: "notification",
        our_matcher: Some("idle_prompt"),
    },
    HookSpec {
        event_key: "Notification",
        matcher: Some("elicitation_dialog"),
        our_event: "notification",
        our_matcher: Some("elicitation_dialog"),
    },
    HookSpec {
        event_key: "Notification",
        matcher: Some("elicitation_response"),
        our_event: "notification",
        our_matcher: Some("elicitation_response"),
    },
    HookSpec {
        event_key: "Stop",
        matcher: None,
        our_event: "stop",
        our_matcher: None,
    },
    HookSpec {
        event_key: "StopFailure",
        matcher: None,
        our_event: "stop-failure",
        our_matcher: None,
    },
    HookSpec {
        event_key: "SessionEnd",
        matcher: None,
        our_event: "session-end",
        our_matcher: None,
    },
    HookSpec {
        event_key: "PreCompact",
        matcher: None,
        our_event: "pre-compact",
        our_matcher: None,
    },
    HookSpec {
        event_key: "PostCompact",
        matcher: None,
        our_event: "post-compact",
        our_matcher: None,
    },
    HookSpec {
        event_key: "SubagentStart",
        matcher: None,
        our_event: "subagent-start",
        our_matcher: None,
    },
    HookSpec {
        event_key: "SubagentStop",
        matcher: None,
        our_event: "subagent-stop",
        our_matcher: None,
    },
];

fn settings_path() -> PathBuf {
    let home = env::var("HOME").expect("HOME must be set");
    PathBuf::from(home).join(".claude").join("settings.json")
}

fn write_settings(path: &PathBuf, root: &Value) -> Result<()> {
    let mut text = serde_json::to_string_pretty(root)?;
    text.push('\n');
    fs::write(path, text)?;
    Ok(())
}

fn hook_command(exe: &str, spec: &HookSpec) -> String {
    let mut cmd = format!("{exe} hook --event {}", spec.our_event);
    if let Some(m) = spec.our_matcher {
        cmd.push_str(&format!(" --matcher {m}"));
    }
    cmd.push_str(" --stdin");
    cmd
}

fn install_binary() -> Result<String> {
    let current_exe = env::current_exe().context("failed to resolve current executable")?;
    let installed_path = paths::installed_binary_path();
    if let Some(parent) = installed_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&current_exe, &installed_path).context("failed to copy binary to install location")?;
    let mut perms = fs::metadata(&installed_path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&installed_path, perms)?;
    Ok(installed_path.to_string_lossy().to_string())
}

fn install_default_config() -> Result<()> {
    let path = paths::config_path();
    if path.exists() {
        println!("Existing config preserved at {}", path.display());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, DEFAULT_CONFIG)?;
    println!("Config created at {}", path.display());
    Ok(())
}

pub fn install() -> Result<()> {
    let exe = install_binary()?;
    install_default_config()?;

    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        "{}".to_string()
    };

    if path.exists() {
        let backup = path.with_file_name("settings.json.backup-kitty-claude-notifier");
        fs::write(&backup, &raw)?;
        println!("Settings backed up to {}", backup.display());
    }

    let mut root: Value = serde_json::from_str(&raw).context("failed to parse settings.json")?;
    let added = merge_hooks(&mut root, &exe)?;
    write_settings(&path, &root)?;
    println!("{added} hook entries added to {}", path.display());
    println!(
        "Note: ensure Kitty has 'allow_remote_control yes' and a 'listen_on' socket configured \
         (needed for kitten @ to reach Kitty from hook processes with no controlling TTY)."
    );
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let marker = paths::installed_binary_path().to_string_lossy().to_string();
    let path = settings_path();

    if path.exists() {
        let raw = fs::read_to_string(&path)?;
        let mut root: Value = serde_json::from_str(&raw)?;

        let removed_any = remove_hooks(&mut root, &marker);

        if removed_any {
            write_settings(&path, &root)?;
            println!("Hooks removed from {}", path.display());
        } else {
            println!(
                "No kitty-claude-notifier hooks found in {}; nothing to remove.",
                path.display()
            );
        }
    } else {
        println!("{} not found; nothing to remove.", path.display());
    }

    let installed_path = paths::installed_binary_path();
    if installed_path.exists() {
        fs::remove_file(&installed_path)?;
        println!("Removed installed binary at {}", installed_path.display());
    }
    if let Some(dir) = installed_path.parent() {
        // Best-effort: only succeeds if now-empty. config.toml lives one
        // level up and is deliberately left in place.
        let _ = fs::remove_dir(dir);
    }
    Ok(())
}

/// Pure JSON transform, no file I/O, so it's directly unit-testable.
/// Appends any of our hook entries not already present under `root.hooks`,
/// creating `hooks` and each event's array as needed, without touching any
/// existing entries (including another tool's). Returns how many were added.
fn merge_hooks(root: &mut Value, exe: &str) -> Result<usize> {
    let root_obj = root
        .as_object_mut()
        .context("settings.json root must be an object")?;
    let hooks = root_obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks
        .as_object_mut()
        .context("settings.json .hooks must be an object")?;

    let mut added = 0;
    for spec in HOOKS {
        let entries = hooks
            .entry(spec.event_key.to_string())
            .or_insert_with(|| json!([]));
        let entries = entries
            .as_array_mut()
            .context("hook entries must be an array")?;

        let command = hook_command(exe, spec);
        let already_present = entries.iter().any(|entry| {
            entry
                .get("hooks")
                .and_then(|h| h.as_array())
                .map(|cmds| {
                    cmds.iter().any(|c| {
                        c.get("command").and_then(|v| v.as_str()) == Some(command.as_str())
                    })
                })
                .unwrap_or(false)
        });
        if already_present {
            continue;
        }

        let mut entry = json!({
            "hooks": [{ "type": "command", "command": command, "async": true }]
        });
        if let Some(m) = spec.matcher {
            entry
                .as_object_mut()
                .unwrap()
                .insert("matcher".to_string(), json!(m));
        }
        entries.push(entry);
        added += 1;
    }
    Ok(added)
}

/// Pure JSON transform, no file I/O. Removes only command entries whose
/// string contains `marker`, from every event's hook array, dropping any
/// event key left with an empty array. Returns whether anything changed.
fn remove_hooks(root: &mut Value, marker: &str) -> bool {
    let mut removed_any = false;
    let Some(hooks) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return false;
    };
    let mut empty_keys = Vec::new();
    for (key, entries) in hooks.iter_mut() {
        let Some(arr) = entries.as_array_mut() else {
            continue;
        };
        arr.retain_mut(|entry| {
            if let Some(cmds) = entry.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                let before = cmds.len();
                cmds.retain(|c| {
                    !c.get("command")
                        .and_then(|v| v.as_str())
                        .map(|s| s.contains(marker))
                        .unwrap_or(false)
                });
                if cmds.len() != before {
                    removed_any = true;
                }
                !cmds.is_empty()
            } else {
                true
            }
        });
        if arr.is_empty() {
            empty_keys.push(key.clone());
        }
    }
    for key in empty_keys {
        hooks.remove(&key);
    }
    removed_any
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = "/home/user/.config/kitty-claude-notifier/bin/kitty-claude-notifier";

    #[test]
    fn merge_adds_all_hooks_into_empty_settings() {
        let mut root = json!({});
        let added = merge_hooks(&mut root, EXE).unwrap();
        assert_eq!(added, HOOKS.len());
        assert_eq!(root["hooks"]["Notification"].as_array().unwrap().len(), 4);
        assert_eq!(root["hooks"]["PreCompact"].as_array().unwrap().len(), 1);
        assert_eq!(root["hooks"]["PostCompact"].as_array().unwrap().len(), 1);
        assert_eq!(root["hooks"]["SubagentStart"].as_array().unwrap().len(), 1);
        assert_eq!(root["hooks"]["SubagentStop"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn merge_is_idempotent() {
        let mut root = json!({});
        merge_hooks(&mut root, EXE).unwrap();
        let added_second_time = merge_hooks(&mut root, EXE).unwrap();
        assert_eq!(added_second_time, 0);
    }

    #[test]
    fn merge_preserves_another_tools_existing_entries() {
        let mut root = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {"hooks": [{"type": "command", "command": "/other/tool --state working --stdin", "async": true}]}
                ],
                "PreToolUse": [
                    {"matcher": "Bash", "hooks": [{"type": "command", "command": "rtk hook claude"}]}
                ]
            }
        });
        merge_hooks(&mut root, EXE).unwrap();

        // The other tool's entry survives untouched...
        let user_prompt = root["hooks"]["UserPromptSubmit"].as_array().unwrap();
        assert_eq!(user_prompt.len(), 2);
        assert_eq!(
            user_prompt[0]["hooks"][0]["command"],
            "/other/tool --state working --stdin"
        );
        // ...and an event we never touch (PreToolUse) is untouched entirely.
        assert_eq!(root["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn remove_deletes_only_our_entries_and_empty_keys() {
        let mut root = json!({});
        merge_hooks(&mut root, EXE).unwrap();
        // Add a foreign entry sharing one of our event keys.
        root["hooks"]["Stop"]
            .as_array_mut()
            .unwrap()
            .push(json!({"hooks": [{"type": "command", "command": "/other/tool --state done"}]}));

        let removed = remove_hooks(&mut root, EXE);
        assert!(removed);

        // Stop keeps the foreign entry, loses ours.
        let stop = root["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1);
        assert_eq!(stop[0]["hooks"][0]["command"], "/other/tool --state done");

        // Events with nothing left (e.g. StopFailure, only ever ours) are
        // removed entirely rather than left as an empty array.
        assert!(root["hooks"].get("StopFailure").is_none());
    }

    #[test]
    fn remove_on_settings_without_our_hooks_is_a_noop() {
        let mut root = json!({
            "hooks": {
                "UserPromptSubmit": [
                    {"hooks": [{"type": "command", "command": "/other/tool --state working"}]}
                ]
            }
        });
        let removed = remove_hooks(&mut root, EXE);
        assert!(!removed);
        assert_eq!(
            root["hooks"]["UserPromptSubmit"].as_array().unwrap().len(),
            1
        );
    }
}
