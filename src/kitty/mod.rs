#[cfg(test)]
pub mod fake;
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
    /// Sets both the focused (`active_bg`) and unfocused (`inactive_bg`)
    /// tab background — Kitty only honors `active_bg` while the tab is
    /// focused, so a background tab needs its own color set explicitly to
    /// show anything at all.
    fn set_tab_color(
        &self,
        target: &WindowTarget,
        active_bg: &str,
        inactive_bg: &str,
    ) -> Result<()>;
    fn get_text(&self, target: &WindowTarget) -> Result<String>;
}

/// Sets title + color together, logging (not propagating) any failure —
/// a Kitty hiccup should never be fatal to the daemon.
pub fn apply(
    client: &dyn KittyClient,
    target: &WindowTarget,
    title: &str,
    active_bg: &str,
    inactive_bg: &str,
) {
    if let Err(e) = client.set_tab_title(target, title) {
        tracing::warn!("set_tab_title failed: {e}");
    }
    if let Err(e) = client.set_tab_color(target, active_bg, inactive_bg) {
        tracing::warn!("set_tab_color failed: {e}");
    }
}
