use std::thread::sleep;
use std::time::Duration;

use clap::Parser;
use kitty_claude_notifier::cli::{Cli, Commands};
use kitty_claude_notifier::kitty::{KittyClient, ProcessKittyClient, WindowTarget};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Hook {
            event,
            matcher,
            stdin,
        } => {
            let client = ProcessKittyClient::new();
            kitty_claude_notifier::hook::run(&event, matcher.as_deref(), stdin, &client)?;
        }
        Commands::Daemon => {
            println!("daemon: not yet implemented");
        }
        Commands::Install => {
            println!("install: not yet implemented");
        }
        Commands::Uninstall => {
            println!("uninstall: not yet implemented");
        }
        Commands::Test => run_test()?,
    }
    Ok(())
}

fn run_test() -> anyhow::Result<()> {
    let client = ProcessKittyClient::new();
    let target = WindowTarget::from_env();
    println!("kitty-claude-notifier: sending test blink (target: {target:?})...");
    client.set_tab_title(&target, "⛔ Perm")?;
    client.set_tab_color(&target, "#ff003c")?;
    sleep(Duration::from_millis(800));
    client.set_tab_title(&target, "")?;
    client.set_tab_color(&target, "NONE")?;
    println!("kitty-claude-notifier: test complete");
    Ok(())
}
