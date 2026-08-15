#[cfg(test)]
pub mod fake;
mod process_client;
mod target;

pub use process_client::ProcessKittyClient;
pub use target::WindowTarget;

use anyhow::Result;

use crate::config::Icon;
use crate::icon;

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
    /// Reads the tab's *current* title (whatever the shell/OS naturally
    /// set it to) so an icon can be prepended without clobbering it.
    ///
    /// Implementations backed by `kitten @ ls` receive a JSON blob that
    /// includes each window's full environment variables and other
    /// sensitive data — extract only the title and never log or retain
    /// the rest of that response.
    fn get_tab_title(&self, target: &WindowTarget) -> Result<String>;
    fn get_text(&self, target: &WindowTarget) -> Result<String>;
}

/// Prepends `icon`'s glyph (in its color) onto the tab's title and sets
/// the tab background, logging (not propagating) any failure — a Kitty
/// hiccup should never be fatal to the daemon.
///
/// The base the icon prepends onto is `icon.text` if the config set one
/// for this state, otherwise the tab's live, shell-set title (fetched via
/// `get_tab_title`, so a fixed `text` override skips that call entirely).
pub fn apply(
    client: &dyn KittyClient,
    target: &WindowTarget,
    icon: &Icon,
    active_bg: &str,
    inactive_bg: &str,
) {
    let base = match &icon.text {
        Some(text) => Ok(text.clone()),
        None => client.get_tab_title(target),
    };
    match base {
        Ok(current) => {
            let new_title = icon::build_title(&icon.glyph, &icon.color, &current);
            if let Err(e) = client.set_tab_title(target, &new_title) {
                tracing::warn!("set_tab_title failed: {e}");
            }
        }
        Err(e) => tracing::warn!("get_tab_title failed: {e}"),
    }
    if let Err(e) = client.set_tab_color(target, active_bg, inactive_bg) {
        tracing::warn!("set_tab_color failed: {e}");
    }
}

/// Strips any icon this tool previously applied (restoring the tab's
/// natural title) and clears the tab background. Used on session cleanup.
pub fn clear(client: &dyn KittyClient, target: &WindowTarget) {
    match client.get_tab_title(target) {
        Ok(current) => {
            let stripped = icon::strip_icon_prefix(&current);
            if let Err(e) = client.set_tab_title(target, stripped) {
                tracing::warn!("set_tab_title failed: {e}");
            }
        }
        Err(e) => tracing::warn!("get_tab_title failed: {e}"),
    }
    if let Err(e) = client.set_tab_color(target, "NONE", "NONE") {
        tracing::warn!("set_tab_color failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fake::{Call, FakeKittyClient};

    fn icon_without_text() -> Icon {
        Icon {
            glyph: "●".to_string(),
            color: "#ffffff".to_string(),
            text: None,
        }
    }

    fn icon_with_text(text: &str) -> Icon {
        Icon {
            glyph: "●".to_string(),
            color: "#ffffff".to_string(),
            text: Some(text.to_string()),
        }
    }

    #[test]
    fn without_text_override_prepends_onto_the_live_title() {
        let fake = FakeKittyClient::new(vec![]).with_initial_title("my project");
        let target = WindowTarget::Id("1".to_string());
        apply(&fake, &target, &icon_without_text(), "#000000", "#111111");

        assert!(fake.calls().contains(&Call::GetTabTitle(target.clone())));
        assert_eq!(
            fake.last_title(),
            Some(icon::build_title("●", "#ffffff", "my project"))
        );
    }

    #[test]
    fn with_text_override_skips_fetching_the_live_title() {
        // A live title is seeded but must be ignored entirely — and never
        // even fetched — when a fixed text override is configured.
        let fake = FakeKittyClient::new(vec![]).with_initial_title("my project");
        let target = WindowTarget::Id("1".to_string());
        apply(
            &fake,
            &target,
            &icon_with_text("NEEDS APPROVAL"),
            "#000000",
            "#111111",
        );

        assert!(!fake.calls().contains(&Call::GetTabTitle(target.clone())));
        assert_eq!(
            fake.last_title(),
            Some(icon::build_title("●", "#ffffff", "NEEDS APPROVAL"))
        );
    }
}
