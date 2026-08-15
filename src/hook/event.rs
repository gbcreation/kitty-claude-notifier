use crate::state::State;

/// Maps a Claude Code hook (event name, Notification matcher) pair to one of
/// our states — the same mapping the bash tool's install.sh bakes into each
/// hook's command line, made explicit and testable here instead.
///
/// Returns `None` for events we don't turn into a tab state (e.g.
/// `session-end`, which the caller handles as a cleanup instead).
pub fn resolve_state(event: &str, matcher: Option<&str>) -> Option<State> {
    match event {
        "user-prompt-submit" => Some(State::Working),
        "notification" => match matcher {
            Some("permission_prompt") => Some(State::Permission),
            Some("idle_prompt") | Some("elicitation_dialog") => Some(State::Waiting),
            Some("elicitation_response") => Some(State::Working),
            _ => None,
        },
        "stop" => Some(State::Done),
        "stop-failure" | "post-tool-use-failure" => Some(State::Error),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_events() {
        assert_eq!(resolve_state("user-prompt-submit", None), Some(State::Working));
        assert_eq!(resolve_state("stop", None), Some(State::Done));
        assert_eq!(resolve_state("stop-failure", None), Some(State::Error));
        assert_eq!(resolve_state("post-tool-use-failure", None), Some(State::Error));
    }

    #[test]
    fn maps_notification_matchers() {
        assert_eq!(
            resolve_state("notification", Some("permission_prompt")),
            Some(State::Permission)
        );
        assert_eq!(resolve_state("notification", Some("idle_prompt")), Some(State::Waiting));
        assert_eq!(
            resolve_state("notification", Some("elicitation_dialog")),
            Some(State::Waiting)
        );
        assert_eq!(
            resolve_state("notification", Some("elicitation_response")),
            Some(State::Working)
        );
    }

    #[test]
    fn unknown_event_or_matcher_yields_none() {
        assert_eq!(resolve_state("session-end", None), None);
        assert_eq!(resolve_state("notification", None), None);
        assert_eq!(resolve_state("notification", Some("bogus")), None);
        assert_eq!(resolve_state("bogus", None), None);
    }
}
