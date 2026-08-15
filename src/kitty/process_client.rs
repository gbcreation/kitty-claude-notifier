use std::env;
use std::process::{Command, Output};

use anyhow::{Context, Result};

use super::{KittyClient, TabInfo, WindowTarget};

/// Real Kitty IPC — shells out to `kitten @`, always via the configured
/// KITTY_LISTEN_ON socket when present rather than relying on the calling
/// process having a controlling TTY (which hook subprocesses often lack).
pub struct ProcessKittyClient;

impl ProcessKittyClient {
    pub fn new() -> Self {
        Self
    }

    fn run(&self, args: &[&str]) -> Result<Output> {
        let mut cmd = Command::new("kitten");
        cmd.arg("@");
        if let Ok(listen_on) = env::var("KITTY_LISTEN_ON")
            && !listen_on.is_empty()
        {
            cmd.args(["--to", &listen_on]);
        }
        cmd.args(args);
        let output = cmd.output().context("failed to spawn kitten")?;
        if !output.status.success() {
            anyhow::bail!(
                "kitten @ {} exited with {}: {}",
                args.join(" "),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(output)
    }
}

impl Default for ProcessKittyClient {
    fn default() -> Self {
        Self::new()
    }
}

impl KittyClient for ProcessKittyClient {
    fn set_tab_title(&self, target: &WindowTarget, title: &str) -> Result<()> {
        self.run(&["set-tab-title", "--match", &target.match_expr(), title])?;
        Ok(())
    }

    fn set_tab_color(
        &self,
        target: &WindowTarget,
        active_bg: &str,
        inactive_bg: &str,
    ) -> Result<()> {
        let active_spec = format!("active_bg={active_bg}");
        let inactive_spec = format!("inactive_bg={inactive_bg}");
        self.run(&[
            "set-tab-color",
            "--match",
            &target.match_expr(),
            &active_spec,
            &inactive_spec,
        ])?;
        Ok(())
    }

    fn get_tab_info(&self, target: &WindowTarget) -> Result<TabInfo> {
        let output = self.run(&["ls", "--match", &target.match_expr()])?;
        // `kitten @ ls` returns each matched window's full environment
        // variables (including secrets) and other sensitive process
        // details alongside the title. Parse just enough to extract the
        // title and focus state and drop the rest immediately — never log
        // `output` or the parsed value.
        let parsed: serde_json::Value =
            serde_json::from_slice(&output.stdout).context("failed to parse kitten @ ls output")?;
        let tab = parsed
            .as_array()
            .and_then(|os_windows| os_windows.first())
            .and_then(|w| w.get("tabs"))
            .and_then(|tabs| tabs.as_array())
            .and_then(|tabs| tabs.first())
            .context("no matching tab found in kitten @ ls output")?;
        let title = tab
            .get("title")
            .and_then(|t| t.as_str())
            .map(str::to_string)
            .context("no title found in kitten @ ls output")?;
        let is_focused = tab
            .get("is_focused")
            .and_then(|f| f.as_bool())
            .unwrap_or(false);
        Ok(TabInfo { title, is_focused })
    }

    fn get_text(&self, target: &WindowTarget) -> Result<String> {
        let output = self.run(&["get-text", "--match", &target.match_expr()])?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}
