use std::thread::sleep;
use std::time::Duration;

use clap::Parser;
use kitty_claude_notifier::cli::{Cli, Commands};
use kitty_claude_notifier::config::Config;
use kitty_claude_notifier::kitty::{KittyClient, ProcessKittyClient, WindowTarget};
use kitty_claude_notifier::paths;
use kitty_claude_notifier::state::State;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Hook {
            event,
            matcher,
            stdin,
        } => {
            kitty_claude_notifier::hook::run(
                &event,
                matcher.as_deref(),
                stdin,
                &paths::daemon_socket_path(),
            )?;
        }
        Commands::Daemon => {
            let config = Config::load(&paths::config_path())?;
            kitty_claude_notifier::daemon::run(&paths::daemon_socket_path(), config)?;
        }
        Commands::Install => {
            println!("install: not yet implemented");
        }
        Commands::Uninstall => {
            println!("uninstall: not yet implemented");
        }
        Commands::Test => {
            let config = Config::load(&paths::config_path())?;
            run_test(&config)?;
        }
    }
    Ok(())
}

fn run_test(config: &Config) -> anyhow::Result<()> {
    let client = ProcessKittyClient::new();
    let target = WindowTarget::from_env();
    println!("kitty-claude-notifier: sending test blink (target: {target:?})...");
    client.set_tab_title(&target, &config.title_for(State::Permission))?;
    client.set_tab_color(&target, &config.color_for(State::Permission))?;
    sleep(Duration::from_millis(800));
    client.set_tab_title(&target, "")?;
    client.set_tab_color(&target, "NONE")?;
    println!("kitty-claude-notifier: test complete");
    Ok(())
}
