mod event;
mod payload;

pub use event::resolve_state;
pub use payload::HookPayload;

use std::io::{self, Read};

use anyhow::Result;

use crate::kitty::{KittyClient, WindowTarget};

/// Handles `hook --event <name> [--matcher <m>] [--stdin]`. For now (no
/// daemon yet) this applies the tab update directly — replicating the bash
/// tool's core per-invocation behavior before session state / timers /
/// resume-detection are introduced.
pub fn run(
    event: &str,
    matcher: Option<&str>,
    read_stdin: bool,
    client: &dyn KittyClient,
) -> Result<()> {
    let raw = if read_stdin {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf).unwrap_or(0);
        buf
    } else {
        String::new()
    };
    let _payload = HookPayload::parse(&raw);

    let target = WindowTarget::from_env();

    match resolve_state(event, matcher) {
        Some(state) => {
            client.set_tab_title(&target, state.default_title())?;
            client.set_tab_color(&target, state.default_color())?;
        }
        None if event == "session-end" => {
            client.set_tab_title(&target, "")?;
            client.set_tab_color(&target, "NONE")?;
        }
        None => {
            // Unrecognized (event, matcher) pair — no-op rather than an
            // error, since we intentionally don't map every Claude Code
            // hook event to a tab state.
        }
    }
    Ok(())
}
