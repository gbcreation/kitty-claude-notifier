use std::sync::Mutex;

use anyhow::Result;

use super::{KittyClient, WindowTarget};

#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    SetTabTitle(WindowTarget, String),
    /// (target, active_bg, inactive_bg)
    SetTabColor(WindowTarget, String, String),
    GetTabTitle(WindowTarget),
    GetText(WindowTarget),
}

/// In-memory `KittyClient` for deterministic unit tests — records every
/// call and returns scripted `get_text` responses, no subprocess involved.
///
/// `tab_title` behaves like a real tab: `set_tab_title` updates it and
/// `get_tab_title` reads it back, so a test can verify icon-stripping
/// across repeated `apply()` calls the same way the real round-trip
/// through Kitty would.
pub struct FakeKittyClient {
    calls: Mutex<Vec<Call>>,
    /// Successive get_text responses, consumed one per call; the last
    /// entry repeats once exhausted (so a test can under-specify the tail).
    get_text_script: Mutex<Vec<String>>,
    tab_title: Mutex<String>,
}

impl FakeKittyClient {
    pub fn new(get_text_script: Vec<&str>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            get_text_script: Mutex::new(get_text_script.into_iter().map(String::from).collect()),
            tab_title: Mutex::new(String::new()),
        }
    }

    /// Seeds the tab's "natural" title as if the shell had already set it,
    /// before any `apply()` call prepends an icon onto it.
    pub fn with_initial_title(self, title: &str) -> Self {
        *self.tab_title.lock().unwrap() = title.to_string();
        self
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    pub fn last_title(&self) -> Option<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|c| match c {
                Call::SetTabTitle(_, t) => Some(t.clone()),
                _ => None,
            })
    }

    /// Returns the last `(active_bg, inactive_bg)` set, if any.
    pub fn last_colors(&self) -> Option<(String, String)> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|c| match c {
                Call::SetTabColor(_, active, inactive) => Some((active.clone(), inactive.clone())),
                _ => None,
            })
    }
}

impl KittyClient for FakeKittyClient {
    fn set_tab_title(&self, target: &WindowTarget, title: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(Call::SetTabTitle(target.clone(), title.to_string()));
        *self.tab_title.lock().unwrap() = title.to_string();
        Ok(())
    }

    fn set_tab_color(
        &self,
        target: &WindowTarget,
        active_bg: &str,
        inactive_bg: &str,
    ) -> Result<()> {
        self.calls.lock().unwrap().push(Call::SetTabColor(
            target.clone(),
            active_bg.to_string(),
            inactive_bg.to_string(),
        ));
        Ok(())
    }

    fn get_tab_title(&self, target: &WindowTarget) -> Result<String> {
        self.calls
            .lock()
            .unwrap()
            .push(Call::GetTabTitle(target.clone()));
        Ok(self.tab_title.lock().unwrap().clone())
    }

    fn get_text(&self, target: &WindowTarget) -> Result<String> {
        self.calls
            .lock()
            .unwrap()
            .push(Call::GetText(target.clone()));
        let mut script = self.get_text_script.lock().unwrap();
        if script.len() > 1 {
            Ok(script.remove(0))
        } else {
            Ok(script.first().cloned().unwrap_or_default())
        }
    }
}
