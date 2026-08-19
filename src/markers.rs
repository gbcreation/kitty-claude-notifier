/// How many trailing lines of `kitten @ get-text` output to check. Bounds
/// matching to the tail of the screen rather than requiring exact
/// structure, so a resized/reflowed/actively-scrolling terminal returning
/// partial content doesn't break detection.
const TAIL_LINES: usize = 40;

/// Case-insensitive substring match: does any marker appear in the tail of
/// the given screen text?
pub fn any_present(screen_text: &str, markers: &[String]) -> bool {
    let tail = tail_lines(screen_text, TAIL_LINES).to_lowercase();
    markers.iter().any(|m| tail.contains(&m.to_lowercase()))
}

fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_marker_case_insensitively() {
        let text = "some output\nDo You Want To Proceed?\n❯ 1. Yes\n2. No";
        let markers = vec!["do you want to proceed?".to_string()];
        assert!(any_present(text, &markers));
    }

    #[test]
    fn absent_marker_not_detected() {
        let text = "just some normal output\nnothing special here";
        let markers = vec!["do you want to proceed?".to_string()];
        assert!(!any_present(text, &markers));
    }

    #[test]
    fn only_checks_tail_lines() {
        let mut lines: Vec<String> = (0..100).map(|i| format!("line {i}")).collect();
        lines[0] = "do you want to proceed?".to_string(); // far outside the tail window
        let text = lines.join("\n");
        let markers = vec!["do you want to proceed?".to_string()];
        assert!(!any_present(&text, &markers));
    }

    /// Regression: the shipped default marker used to be the literal
    /// "❯ 1. yes", which missed permission prompts whose suggested first
    /// option isn't worded "yes" (e.g. a custom or context-specific
    /// suggestion). The numbered-menu prefix alone is the stable,
    /// structural part.
    #[test]
    fn numbered_menu_prefix_matches_regardless_of_the_suggested_option_text() {
        let text = "Do you want to proceed?\n❯ 1. Always allow this command\n  2. No";
        let markers = vec!["❯ 1. ".to_string()];
        assert!(any_present(text, &markers));
    }
}
