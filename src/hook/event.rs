use crate::state::State;

/// Maps a Claude Code hook (event name, Notification matcher) pair to one of
/// our states, the same mapping the bash tool's install.sh bakes into each
/// hook's command line, made explicit and testable here instead.
///
/// Returns `None` for events we don't turn into a tab state (e.g.
/// `session-end`, which the caller handles as a cleanup instead;
/// `post-tool-use-failure`, deliberately not registered at all, see below).
///
/// `post-tool-use-failure` (a single tool call failing) is intentionally
/// *not* mapped to `Error`: Claude routinely recovers from a failed tool
/// call within the same turn without any input from the user, so flagging
/// every one as an error state was mostly noise, not signal, and, worse,
/// the state had no automatic exit (see `daemon::idle_timer`), so a tab
/// could get stuck showing "error" long after Claude had already moved on.
/// `stop-failure` (the whole turn ending in failure) is kept; nothing
/// "tries something else" after that, so it's a real, terminal signal
/// worth surfacing.
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
        "stop-failure" => Some(State::Error),
        "pre-compact" => Some(State::Compacting),
        // Whatever Claude Code does right after compacting (continue
        // generating, ask something, finish the turn) fires its own
        // correct hook shortly anyway; Working is just a safe interim
        // default rather than leaving the tab on Compacting indefinitely.
        "post-compact" => Some(State::Working),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_events() {
        assert_eq!(
            resolve_state("user-prompt-submit", None),
            Some(State::Working)
        );
        assert_eq!(resolve_state("stop", None), Some(State::Done));
        assert_eq!(resolve_state("stop-failure", None), Some(State::Error));
        assert_eq!(resolve_state("pre-compact", None), Some(State::Compacting));
        assert_eq!(resolve_state("post-compact", None), Some(State::Working));
    }

    #[test]
    fn post_tool_use_failure_is_deliberately_not_mapped() {
        // A single tool failing is not surfaced as `Error`. Claude
        // routinely recovers within the same turn without user input.
        assert_eq!(resolve_state("post-tool-use-failure", None), None);
    }

    #[test]
    fn maps_notification_matchers() {
        assert_eq!(
            resolve_state("notification", Some("permission_prompt")),
            Some(State::Permission)
        );
        assert_eq!(
            resolve_state("notification", Some("idle_prompt")),
            Some(State::Waiting)
        );
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
