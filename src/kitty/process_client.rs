use std::env;
use std::process::{Command, Output};

use anyhow::{Context, Result};

use super::{KittyClient, WindowTarget};

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
        if let Ok(listen_on) = env::var("KITTY_LISTEN_ON") {
            if !listen_on.is_empty() {
                cmd.args(["--to", &listen_on]);
            }
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

    fn set_tab_color(&self, target: &WindowTarget, active_bg: &str) -> Result<()> {
        let spec = format!("active_bg={active_bg}");
        self.run(&["set-tab-color", "--match", &target.match_expr(), &spec])?;
        Ok(())
    }

    fn get_text(&self, target: &WindowTarget) -> Result<String> {
        let output = self.run(&["get-text", "--match", &target.match_expr()])?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}
