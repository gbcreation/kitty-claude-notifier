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

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub idle_timeout_secs: u64,
    pub resume_poll_interval_ms: u64,
    /// Per-state title overrides, keyed by State's Display (e.g. "permission").
    pub titles: HashMap<String, String>,
    /// Per-state color overrides, same keys as `titles`.
    pub colors: HashMap<String, ColorSpec>,
    /// Text markers that indicate a permission prompt is still on screen —
    /// used by the resume-detection screen scrape (not yet wired up).
    pub permission_markers: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            idle_timeout_secs: 300,
            resume_poll_interval_ms: 500,
            titles: HashMap::new(),
            colors: HashMap::new(),
            permission_markers: default_permission_markers(),
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

    pub fn title_for(&self, state: State) -> String {
        self.titles
            .get(&state.to_string())
            .cloned()
            .unwrap_or_else(|| state.default_title().to_string())
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
    fn falls_back_to_default_title_when_unset() {
        let cfg = Config::default();
        assert_eq!(cfg.title_for(State::Permission), "⛔ Perm");
    }

    #[test]
    fn falls_back_to_same_default_color_for_both_when_unset() {
        let cfg = Config::default();
        let (active, inactive) = cfg.colors_for(State::Permission);
        assert_eq!(active, "#ff003c");
        assert_eq!(active, inactive);
    }

    #[test]
    fn override_replaces_default() {
        let raw = r#"
            [titles]
            permission = "!! PERM !!"
        "#;
        let cfg: Config = toml::from_str(raw).unwrap();
        assert_eq!(cfg.title_for(State::Permission), "!! PERM !!");
        assert_eq!(cfg.title_for(State::Working), "⚡ Work");
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
        assert_eq!(cfg.titles.len(), 6);
    }
}
