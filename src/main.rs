use clap::Parser;
use kitty_claude_notifier::cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Hook { event, stdin } => {
            println!("hook: event={event} stdin={stdin} (not yet implemented)");
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
        Commands::Test => {
            println!("test: not yet implemented");
        }
    }
}
