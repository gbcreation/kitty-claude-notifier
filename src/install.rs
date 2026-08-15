use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{json, Value};

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
        event_key: "PostToolUseFailure",
        matcher: Some(".*"),
        our_event: "post-tool-use-failure",
        our_matcher: None,
    },
    HookSpec {
        event_key: "SessionEnd",
        matcher: None,
        our_event: "session-end",
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
    fs::copy(&current_exe, &installed_path)
        .context("failed to copy binary to install location")?;
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

        let command = hook_command(&exe, spec);
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

    let mut removed_any = false;
    if path.exists() {
        let raw = fs::read_to_string(&path)?;
        let mut root: Value = serde_json::from_str(&raw)?;

        if let Some(hooks) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
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
                                .map(|s| s.contains(&marker))
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
        }

        if removed_any {
            write_settings(&path, &root)?;
            println!("Hooks removed from {}", path.display());
        } else {
            println!("No kitty-claude-notifier hooks found in {} — nothing to remove.", path.display());
        }
    } else {
        println!("{} not found — nothing to remove.", path.display());
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
