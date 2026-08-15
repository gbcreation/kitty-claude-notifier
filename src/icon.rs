//! Builds/strips the colored icon prefix prepended to a tab's existing
//! title. Kitty's tab bar renders raw ANSI truecolor escape codes embedded
//! directly in a title string (verified against a real Kitty instance —
//! this isn't documented behavior to rely on blindly). Emoji are NOT
//! usable here: they carry their own embedded color glyph (COLR/emoji
//! font tables) and ignore SGR foreground color entirely, so icons must
//! be plain Unicode symbols or Nerd Font glyphs, not emoji.

const PREFIX_START: &str = "\x1b[38;2;";
const RESET: &str = "\x1b[39m";

/// Prepends `glyph` (colored via `color_hex`, e.g. "#ff003c") onto
/// `base_title`, first stripping any icon prefix this tool previously
/// applied so repeated updates don't stack multiple icons.
pub fn build_title(glyph: &str, color_hex: &str, base_title: &str) -> String {
    let stripped = strip_icon_prefix(base_title);
    let separator = if stripped.is_empty() { "" } else { " " };
    match parse_hex_color(color_hex) {
        Some((r, g, b)) => format!("{PREFIX_START}{r};{g};{b}m{glyph}{RESET}{separator}{stripped}"),
        None => format!("{glyph}{separator}{stripped}"),
    }
}

/// Removes a leading colored-icon prefix matching the exact wrapper
/// `build_title` produces, if present. Titles without one pass through
/// unchanged; a malformed/partial prefix (missing its reset sequence) is
/// also left unchanged rather than risk mangling real title text.
pub fn strip_icon_prefix(title: &str) -> &str {
    let Some(rest) = title.strip_prefix(PREFIX_START) else {
        return title;
    };
    match rest.find(RESET) {
        Some(pos) => {
            let after = &rest[pos + RESET.len()..];
            after.strip_prefix(' ').unwrap_or(after)
        }
        None => title,
    }
}

fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_colored_prefix_over_plain_title() {
        let title = build_title("●", "#ff0000", "my project");
        assert_eq!(title, "\x1b[38;2;255;0;0m●\x1b[39m my project");
    }

    #[test]
    fn strips_previous_icon_before_prepending_a_new_one() {
        let first = build_title("●", "#ff0000", "my project");
        let second = build_title("▲", "#00ff00", &first);
        assert_eq!(second, "\x1b[38;2;0;255;0m▲\x1b[39m my project");
        // Only one icon present, not two stacked.
        assert_eq!(second.matches('\u{1b}').count(), 2); // one CSI start + one reset
    }

    #[test]
    fn empty_base_title_yields_just_the_icon_no_trailing_space() {
        let title = build_title("●", "#ff0000", "");
        assert_eq!(title, "\x1b[38;2;255;0;0m●\x1b[39m");
    }

    #[test]
    fn round_trips_correctly_when_base_title_is_empty() {
        // Regression: build_title's empty-base case must produce output
        // strip_icon_prefix can still parse back out (no dangling space
        // to depend on finding).
        let built = build_title("●", "#ff0000", "");
        assert_eq!(strip_icon_prefix(&built), "");
    }

    #[test]
    fn invalid_hex_falls_back_to_uncolored_icon() {
        let title = build_title("●", "not-a-color", "my project");
        assert_eq!(title, "● my project");
    }

    #[test]
    fn title_without_prefix_is_unaffected_by_stripping() {
        assert_eq!(strip_icon_prefix("my project"), "my project");
    }

    #[test]
    fn malformed_prefix_missing_reset_is_left_alone() {
        let malformed = "\x1b[38;2;255;0;0mno reset sequence here";
        assert_eq!(strip_icon_prefix(malformed), malformed);
    }
}
