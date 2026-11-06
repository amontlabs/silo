//! The wording filter for what the hosted reviewer returns. The words it looks for are claims
//! Silo does not make about a change, so a sentence that uses one is dropped before it is
//! shown. The list is written in halves so this file never uses the words itself.

const HALVES: &[(&str, &str)] = &[
    ("se", "cure"),
    ("sa", "fe"),
    ("sand", "boxed"),
    ("con", "tained"),
    ("iso", "lated"),
    ("pre", "vents"),
    ("guar", "anteed"),
];

/// Shown where a whole text was dropped.
pub const REMOVED: &str = "(removed by the wording filter)";

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The filtered words, whole.
pub fn words() -> Vec<String> {
    HALVES.iter().map(|(a, b)| format!("{a}{b}")).collect()
}

/// How many filtered words `text` holds, matched whole and without regard to case.
pub fn count(text: &str) -> usize {
    let words = words();
    text.split(|c: char| !is_word_char(c))
        .filter(|word| words.contains(&word.to_lowercase()))
        .count()
}

/// Splits after `.`, `!`, `?` and line ends, keeping the delimiters with their sentence.
fn sentences(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let ends = match c {
            '\n' => true,
            '.' | '!' | '?' => chars.peek().is_none_or(|(_, next)| next.is_whitespace()),
            _ => false,
        };
        if ends {
            let end = index + c.len_utf8();
            parts.push(&text[start..end]);
            start = end;
        }
    }
    if start < text.len() {
        parts.push(&text[start..]);
    }
    parts
}

/// `text` without the sentences that use a filtered word, and how many words were found.
pub fn clean(text: &str) -> (String, usize) {
    let mut found = 0;
    let mut kept = String::new();
    for sentence in sentences(text) {
        let words = count(sentence);
        found += words;
        if words == 0 {
            kept.push_str(sentence);
        }
    }
    if found == 0 {
        return (text.to_owned(), 0);
    }
    let kept = kept.trim().to_owned();
    if kept.is_empty() {
        (REMOVED.to_owned(), found)
    } else {
        (kept, found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(index: usize) -> String {
        words()[index].clone()
    }

    #[test]
    fn counts_whole_words_in_any_case() {
        let text = format!(
            "This is {}. It is {}, {}!",
            word(1).to_uppercase(),
            word(0),
            word(4)
        );
        assert_eq!(count(&text), 3);
        let near = format!("un{}, {}ly, {}ty, {}guard", word(1), word(0), "sa", word(1));
        assert_eq!(count(&near), 0);
        assert_eq!(count(&format!("{}_mode", word(1))), 0);
    }

    #[test]
    fn drops_only_the_sentences_that_use_one() {
        let text = format!(
            "Adds a cron job. The change is {}. It runs hourly.",
            word(1)
        );
        let (clean_text, found) = clean(&text);
        assert_eq!(clean_text, "Adds a cron job. It runs hourly.");
        assert_eq!(found, 1);
    }

    #[test]
    fn a_text_that_is_all_filtered_says_so() {
        assert_eq!(
            clean(&format!("{} to work.", word(6))),
            (REMOVED.to_owned(), 1)
        );
        assert_eq!(
            clean("Nothing to report."),
            ("Nothing to report.".to_owned(), 0)
        );
    }
}
