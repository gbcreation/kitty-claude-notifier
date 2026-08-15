use std::sync::Mutex;

use anyhow::Result;

use super::{KittyClient, WindowTarget};

#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    SetTabTitle(WindowTarget, String),
    SetTabColor(WindowTarget, String),
    GetText(WindowTarget),
}

/// In-memory `KittyClient` for deterministic unit tests — records every
/// call and returns scripted `get_text` responses, no subprocess involved.
pub struct FakeKittyClient {
    calls: Mutex<Vec<Call>>,
    /// Successive get_text responses, consumed one per call; the last
    /// entry repeats once exhausted (so a test can under-specify the tail).
    get_text_script: Mutex<Vec<String>>,
}

impl FakeKittyClient {
    pub fn new(get_text_script: Vec<&str>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            get_text_script: Mutex::new(get_text_script.into_iter().map(String::from).collect()),
        }
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
}

impl KittyClient for FakeKittyClient {
    fn set_tab_title(&self, target: &WindowTarget, title: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(Call::SetTabTitle(target.clone(), title.to_string()));
        Ok(())
    }

    fn set_tab_color(&self, target: &WindowTarget, active_bg: &str) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push(Call::SetTabColor(target.clone(), active_bg.to_string()));
        Ok(())
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
