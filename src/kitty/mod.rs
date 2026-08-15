mod process_client;
mod target;

pub use process_client::ProcessKittyClient;
pub use target::WindowTarget;

use anyhow::Result;

/// Kitty IPC surface, abstracted so the reactive resume-detection logic
/// (added later) can be tested against a scripted fake instead of a real
/// Kitty instance.
pub trait KittyClient {
    fn set_tab_title(&self, target: &WindowTarget, title: &str) -> Result<()>;
    fn set_tab_color(&self, target: &WindowTarget, active_bg: &str) -> Result<()>;
    fn get_text(&self, target: &WindowTarget) -> Result<String>;
}

/// Sets title + color together, logging (not propagating) any failure —
/// a Kitty hiccup should never be fatal to the daemon.
pub fn apply(client: &dyn KittyClient, target: &WindowTarget, title: &str, color: &str) {
    if let Err(e) = client.set_tab_title(target, title) {
        tracing::warn!("set_tab_title failed: {e}");
    }
    if let Err(e) = client.set_tab_color(target, color) {
        tracing::warn!("set_tab_color failed: {e}");
    }
}
