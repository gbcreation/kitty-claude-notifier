use serde::Deserialize;

/// The subset of Claude Code's hook JSON we currently care about. Fields are
/// optional and unknown ones are ignored — hook payloads vary by event type
/// and we never want a shape mismatch to break tab updates.
#[derive(Debug, Deserialize, Default)]
pub struct HookPayload {
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
}

impl HookPayload {
    /// Best-effort parse: invalid or empty input yields an empty payload
    /// rather than an error, matching the bash tool's forgiving jq usage.
    pub fn parse(raw: &str) -> Self {
        if raw.trim().is_empty() {
            return Self::default();
        }
        serde_json::from_str(raw).unwrap_or_default()
    }
}
