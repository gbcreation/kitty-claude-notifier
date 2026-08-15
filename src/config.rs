use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use serde::Deserialize;

use crate::state::State;

/// A per-state color override: either a plain string (used for both the
/// active and inactive tab background), or a table giving each one
/// independently — e.g. `permission = "#ff003c"` vs.
/// `permission = { active = "#ff003c", inactive = "#7a0020" }`.
#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(untagged)]
pub enum ColorSpec {
    Same(String),
    Different { active: String, inactive: String },
}

impl ColorSpec {
    pub fn active(&self) -> &str {
        match self {
            ColorSpec::Same(c) => c,
            ColorSpec::Different { active, .. } => active,
        }
    }

    pub fn inactive(&self) -> &str {
        match self {
            ColorSpec::Same(c) => c,
            ColorSpec::Different { inactive, .. } => inactive,
        }
    }
}

/// A per-state icon override: the glyph prepended to the tab's title, and
/// the color it's rendered in. Must be a plain (non-emoji) Unicode symbol
/// or Nerd Font glyph — emoji ignore ANSI foreground color.
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct IconSpec {
    pub glyph: String,
    pub color: String,
    /// Fixed text to prepend the icon onto instead of the tab's live,
    /// shell-set title — the old per-state custom-title behavior, now
    /// combined with the icon rather than replaced by it. Omit to keep
    /// prepending onto whatever the tab's title naturally is.
    #[serde(default)]
    pub text: Option<String>,
}

/// Resolved icon for a state: glyph, color, and — if configured — fixed
/// text to use as the title's base instead of fetching it live.
#[derive(Debug, PartialEq)]
pub struct Icon {
    pub glyph: String,
    pub color: String,
    pub text: Option<String>,
}

/// Custom sound file overrides — omit either to use the built-in default
/// (sourced from herdrdev/herdr, see assets/sounds/NOTICE.md).
#[derive(Debug, Deserialize, PartialEq, Clone, Default)]
#[serde(default)]
pub struct SoundPaths {
    pub request: Option<String>,
    pub done: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub idle_timeout_secs: u64,
    pub resume_poll_interval_ms: u64,
    /// Per-state icon overrides, keyed by State's Display (e.g. "permission").
    pub icons: HashMap<String, IconSpec>,
    /// Per-state tab-background color overrides, same keys as `icons`.
    pub colors: HashMap<String, ColorSpec>,
    /// Text markers that indicate a permission prompt is still on screen —
    /// used by the resume-detection screen scrape (not yet wired up).
    pub permission_markers: Vec<String>,
    /// Off by default — a bigger behavior change than a visual tweak.
    pub sound_enabled: bool,
    /// Off by default — a sound normally doesn't play for a tab you're
    /// already looking at (its `is_focused` tab is true). Set true to
    /// play regardless of focus.
    pub sound_play_when_focused: bool,
    pub sounds: SoundPaths,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            idle_timeout_secs: 300,
            resume_poll_interval_ms: 500,
            icons: HashMap::new(),
            colors: HashMap::new(),
            permission_markers: default_permission_markers(),
            sound_enabled: false,
            sound_play_when_focused: false,
            sounds: SoundPaths::default(),
        }
    }
}

impl Config {
    /// Loads `path`, falling back to defaults entirely if it doesn't exist.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&raw)?)
    }

    pub fn icon_for(&self, state: State) -> Icon {
        match self.icons.get(&state.to_string()) {
            Some(spec) => Icon {
                glyph: spec.glyph.clone(),
                color: spec.color.clone(),
                text: spec.text.clone(),
            },
            None => Icon {
                glyph: state.default_icon_glyph().to_string(),
                color: state.default_icon_color().to_string(),
                text: None,
            },
        }
    }

    /// Returns `(active_bg, inactive_bg)` for `state` — the built-in
    /// default uses the same color for both; a config override may split
    /// them via `ColorSpec::Different`.
    pub fn colors_for(&self, state: State) -> (String, String) {
        match self.colors.get(&state.to_string()) {
            Some(spec) => (spec.active().to_string(), spec.inactive().to_string()),
            None => {
                let default = state.default_color().to_string();
                (default.clone(), default)
            }
        }
    }
}

fn default_permission_markers() -> Vec<String> {
    vec![
        "do you want to proceed?".to_string(),
        "❯ 1. yes".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let cfg = Config::load(Path::new("/nonexistent/config.toml")).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn falls_back_to_default_icon_when_unset() {
        let cfg = Config::default();
        let icon = cfg.icon_for(State::Permission);
        assert_eq!(icon.glyph, "▲");
        assert_eq!(icon.color, "#ffffff");
        assert_eq!(icon.text, None);
    }

    #[test]
    fn falls_back_to_same_default_color_for_both_when_unset() {
        let cfg = Config::default();
        let (active, inactive) = cfg.colors_for(State::Permission);
        assert_eq!(active, "#ff003c");
        assert_eq!(active, inactive);
    }

    #[test]
    fn icon_override_replaces_default() {
        let raw = r##"
            [icons.permission]
            glyph = "!"
            color = "#123456"
        "##;
        let cfg: Config = toml::from_str(raw).unwrap();
        let icon = cfg.icon_for(State::Permission);
        assert_eq!(icon.glyph, "!");
        assert_eq!(icon.color, "#123456");
        assert_eq!(icon.text, None);

        // Unrelated states are unaffected and still fall back to default.
        let working = cfg.icon_for(State::Working);
        assert_eq!(working.glyph, State::Working.default_icon_glyph());
        assert_eq!(working.color, State::Working.default_icon_color());
    }

    #[test]
    fn icon_text_override_is_used_instead_of_the_live_title() {
        let raw = r##"
            [icons.permission]
            glyph = "▲"
            color = "#ffffff"
            text = "NEEDS APPROVAL"
        "##;
        let cfg: Config = toml::from_str(raw).unwrap();
        let icon = cfg.icon_for(State::Permission);
        assert_eq!(icon.text.as_deref(), Some("NEEDS APPROVAL"));
    }

    #[test]
    fn sound_is_disabled_by_default() {
        assert!(!Config::default().sound_enabled);
        assert!(!Config::default().sound_play_when_focused);
        assert_eq!(Config::default().sounds, SoundPaths::default());
    }

    #[test]
    fn sound_config_parses() {
        let raw = r##"
            sound_enabled = true
            sound_play_when_focused = true

            [sounds]
            request = "/tmp/custom-request.mp3"
        "##;
        let cfg: Config = toml::from_str(raw).unwrap();
        assert!(cfg.sound_enabled);
        assert!(cfg.sound_play_when_focused);
        assert_eq!(
            cfg.sounds.request.as_deref(),
            Some("/tmp/custom-request.mp3")
        );
        assert_eq!(cfg.sounds.done, None);
    }

    #[test]
    fn plain_string_color_override_applies_to_both() {
        let raw = r##"
            [colors]
            permission = "#123456"
        "##;
        let cfg: Config = toml::from_str(raw).unwrap();
        assert_eq!(
            cfg.colors_for(State::Permission),
            ("#123456".to_string(), "#123456".to_string())
        );
    }

    #[test]
    fn table_color_override_splits_active_and_inactive() {
        let raw = r##"
            [colors.permission]
            active = "#ff003c"
            inactive = "#7a0020"
        "##;
        let cfg: Config = toml::from_str(raw).unwrap();
        assert_eq!(
            cfg.colors_for(State::Permission),
            ("#ff003c".to_string(), "#7a0020".to_string())
        );
        // Unrelated states are unaffected and still fall back to default.
        assert_eq!(
            cfg.colors_for(State::Working),
            (
                State::Working.default_color().to_string(),
                State::Working.default_color().to_string()
            )
        );
    }

    /// Regression test: a prior version of config/default.toml placed
    /// `permission_markers` after the `[colors]` table header, so TOML
    /// silently nested it *inside* `colors` instead of at the document
    /// root — `colors: HashMap<String, ColorSpec>` then failed to parse
    /// the array as a color, and the daemon refused to start entirely.
    #[test]
    fn shipped_default_config_parses() {
        let raw = include_str!("../config/default.toml");
        let cfg: Config = toml::from_str(raw).expect("config/default.toml must parse");
        assert!(!cfg.permission_markers.is_empty());
        assert_eq!(cfg.colors.len(), 6);
        assert_eq!(cfg.icons.len(), 6);
    }
}
