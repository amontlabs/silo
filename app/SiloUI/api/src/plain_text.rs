//! Text from a computer's agent that a person reads: a terminal or a window must never take it
//! for Silo's own words or for commands to itself.

/// `input` without control characters, terminal escape sequences, or the characters that make
/// text run in another direction or hide. A line break or a tab becomes a space.
pub fn plain_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => skip_escape(&mut chars),
            '\n' | '\r' | '\t' | '\u{2028}' | '\u{2029}' => out.push(' '),
            c if c.is_control() => {}
            '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// `input` made plain, at most `max` characters; a longer text ends with "…".
pub fn plain_text_within(input: &str, max: usize) -> String {
    let plain = plain_text(input);
    if plain.chars().count() <= max {
        return plain;
    }
    let mut cut: String = plain.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// Skips the rest of an escape sequence whose introducer was just read.
fn skip_escape(chars: &mut std::str::Chars<'_>) {
    match chars.next() {
        // A control sequence ends at its first character from `@` to `~`.
        Some('[') => {
            for c in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    break;
                }
            }
        }
        // A string sequence ends at the bell or at an escape and a backslash.
        Some(']' | 'P' | 'X' | '^' | '_') => {
            let mut escaped = false;
            for c in chars.by_ref() {
                if c == '\u{7}' || (escaped && c == '\\') {
                    break;
                }
                escaped = c == '\u{1b}';
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_characters_escapes_and_direction_overrides_are_removed() {
        assert_eq!(plain_text("ok\u{7}\u{0}x"), "okx");
        assert_eq!(plain_text("a\nb\r\nc\td"), "a b  c d");
        assert_eq!(plain_text("\u{1b}[31mred\u{1b}[0m"), "red");
        assert_eq!(plain_text("\u{1b}]0;title\u{7}after"), "after");
        assert_eq!(plain_text("\u{1b}]8;;http://x\u{1b}\\link"), "link");
        assert_eq!(plain_text("safe\u{202E}gpj.exe"), "safegpj.exe");
        assert_eq!(plain_text("a\u{2066}b\u{200B}c\u{FEFF}d"), "abcd");
        assert_eq!(plain_text("Calé 日本 🙂"), "Calé 日本 🙂");
    }

    #[test]
    fn a_long_text_is_cut_with_an_ellipsis() {
        assert_eq!(plain_text_within("abcdef", 6), "abcdef");
        assert_eq!(plain_text_within("abcdefg", 6), "abcde…");
    }
}
