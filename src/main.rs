use std::thread::sleep;
use std::time::Duration;

use clap::Parser;
use kitty_claude_notifier::cli::{Cli, Commands};
use kitty_claude_notifier::config::Config;
use kitty_claude_notifier::kitty::{self, ProcessKittyClient, WindowTarget};
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
            kitty_claude_notifier::daemon::run(&paths::daemon_socket_path(), paths::config_path())?;
        }
        Commands::Install => kitty_claude_notifier::install::install()?,
        Commands::Uninstall => kitty_claude_notifier::install::uninstall()?,
        Commands::Test => {
            // kitty::apply/clear only log (never return errors), so without
            // a subscriber those warnings would be silently dropped,
            // defeating the point of a diagnostic command.
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .init();
            let config = Config::load(&paths::config_path())?;
            run_test(&config);
        }
    }
    Ok(())
}

fn run_test(config: &Config) {
    let client = ProcessKittyClient::new();
    let target = WindowTarget::from_env();
    println!("kitty-claude-notifier: sending test blink (target: {target:?})...");
    let icon = config.icon_for(State::Permission);
    let (active, inactive) = config.colors_for(State::Permission);
    kitty::apply(&client, &target, &icon, &active, &inactive);
    sleep(Duration::from_millis(800));
    kitty::clear(&client, &target);
    println!("kitty-claude-notifier: test complete");
}
