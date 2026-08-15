mod event;
mod payload;

pub use event::resolve_state;
pub use payload::HookPayload;

use std::io::{self, Read};
use std::path::Path;

use anyhow::Result;

use crate::ipc::connect;
use crate::ipc::protocol::{HookMessage, MessageKind};
use crate::kitty::WindowTarget;

/// Handles `hook --event <name> [--matcher <m>] [--stdin]`: resolves the
/// target state and Kitty window here (both only make sense in the hook
/// process's own environment), then forwards a single message to the
/// daemon, spawning it first if needed. The daemon owns all Kitty IPC and
/// session state from here on.
pub fn run(event: &str, matcher: Option<&str>, read_stdin: bool, socket_path: &Path) -> Result<()> {
    let raw = if read_stdin {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf).unwrap_or(0);
        buf
    } else {
        String::new()
    };
    let payload = HookPayload::parse(&raw);
    let target = WindowTarget::from_env();

    let kind = match resolve_state(event, matcher) {
        Some(state) => MessageKind::SetState(state),
        None if event == "session-end" => MessageKind::Cleanup,
        None => return Ok(()),
    };

    let msg = HookMessage {
        session_id: payload.session_id,
        target,
        kind,
    };
    connect::send(socket_path, &msg)
}
