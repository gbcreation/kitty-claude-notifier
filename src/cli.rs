use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "kitty-claude-notifier",
    version,
    about = "Reactive Kitty tab indicators for Claude Code"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Invoked directly by a Claude Code hook; forwards the event to the daemon.
    Hook {
        /// Claude Code's hook event name, e.g. "stop", "notification".
        #[arg(long)]
        event: String,
        /// The Notification matcher that fired, e.g. "permission_prompt".
        #[arg(long)]
        matcher: Option<String>,
        #[arg(long)]
        stdin: bool,
    },
    /// Long-running background process that owns all Kitty IPC and session state.
    Daemon,
    /// Register this tool's hooks into ~/.claude/settings.json.
    Install,
    /// Remove this tool's hooks from ~/.claude/settings.json.
    Uninstall,
    /// Flash a tab title/color directly, with no daemon involved: a smoke test.
    Test,
}
