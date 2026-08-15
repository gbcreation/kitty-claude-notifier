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
