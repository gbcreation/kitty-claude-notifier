use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use serde::Deserialize;

use crate::state::State;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub idle_timeout_secs: u64,
    pub resume_poll_interval_ms: u64,
    /// Per-state title overrides, keyed by State's Display (e.g. "permission").
    pub titles: HashMap<String, String>,
    /// Per-state `active_bg` color overrides, same keys as `titles`.
    pub colors: HashMap<String, String>,
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

    pub fn color_for(&self, state: State) -> String {
        self.colors
            .get(&state.to_string())
            .cloned()
            .unwrap_or_else(|| state.default_color().to_string())
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
    fn override_replaces_default() {
        let raw = r#"
            [titles]
            permission = "!! PERM !!"
        "#;
        let cfg: Config = toml::from_str(raw).unwrap();
        assert_eq!(cfg.title_for(State::Permission), "!! PERM !!");
        assert_eq!(cfg.title_for(State::Working), "⚡ Work");
    }

    /// Regression test: a prior version of config/default.toml placed
    /// `permission_markers` after the `[colors]` table header, so TOML
    /// silently nested it *inside* `colors` instead of at the document
    /// root — `colors: HashMap<String, String>` then failed to parse the
    /// array as a string, and the daemon refused to start entirely.
    #[test]
    fn shipped_default_config_parses() {
        let raw = include_str!("../config/default.toml");
        let cfg: Config = toml::from_str(raw).expect("config/default.toml must parse");
        assert!(!cfg.permission_markers.is_empty());
        assert_eq!(cfg.colors.len(), 6);
        assert_eq!(cfg.titles.len(), 6);
    }
}
