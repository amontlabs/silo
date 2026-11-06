//! The wording filter for what the hosted reviewer returns. The words it looks for are claims
//! Silo does not make about a change, so each use is replaced before the text is shown. The
//! stems are written in halves so this file never uses the words itself.
//!
//! Text is normalised first (NFKC, invisible format characters removed, look-alike Cyrillic
//! and Greek letters folded to Latin), so spacing tricks and homoglyphs do not get past it.

use unicode_normalization::UnicodeNormalization;

/// Prefixes: any word that starts with one is replaced (`safe`, `safely`, `safety`).
const STEMS: &[(&str, &str)] = &[
    ("sa", "fe"),
    ("se", "cur"),
    ("sand", "box"),
    ("iso", "lat"),
    ("guar", "ant"),
    ("pre", "vent"),
];
/// Words matched whole, so `contains` and `container` stay.
const WHOLE: &[(&str, &str)] = &[("con", "tained")];

/// What a filtered word becomes.
pub const REPLACEMENT: &str = "[wording removed]";

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Characters with no visible form: zero-width and bidirectional controls, soft hyphen,
/// byte order mark, word joiner, tag characters and variation selectors.
fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{034F}' | '\u{061C}' | '\u{115F}'..='\u{1160}' | '\u{17B4}'..='\u{17B5}'
        | '\u{180B}'..='\u{180F}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{206F}' | '\u{3164}' | '\u{FE00}'..='\u{FE0F}' | '\u{FEFF}'
        | '\u{FFA0}' | '\u{FFF9}'..='\u{FFFB}' | '\u{E0000}'..='\u{E0FFF}')
}

#[derive(PartialEq, Clone, Copy)]
enum Script {
    Latin,
    Cyrillic,
    Greek,
}

fn script(c: char) -> Option<Script> {
    match c {
        'a'..='z' | 'A'..='Z' => Some(Script::Latin),
        '\u{0400}'..='\u{052F}' => Some(Script::Cyrillic),
        '\u{0370}'..='\u{03FF}' | '\u{1F00}'..='\u{1FFF}' => Some(Script::Greek),
        _ => None,
    }
}

/// The Latin letter a Cyrillic or Greek letter is mistaken for.
fn fold(c: char) -> char {
    match c {
        'а' | 'α' => 'a',
        'е' | 'ε' | 'є' => 'e',
        'о' | 'ο' | 'σ' => 'o',
        'р' | 'ρ' => 'p',
        'с' | 'ѕ' => 's',
        'і' | 'ι' | 'ї' | 'ј' => 'i',
        'к' | 'κ' => 'k',
        'х' | 'χ' => 'x',
        'у' | 'γ' => 'y',
        'ν' | 'υ' => 'v',
        'ԁ' => 'd',
        'һ' => 'h',
        'ӏ' => 'l',
        'ԛ' => 'q',
        'ԝ' => 'w',
        'т' | 'τ' => 't',
        'в' | 'β' => 'b',
        'н' | 'η' => 'n',
        'м' | 'μ' => 'm',
        'з' => '3',
        other => other,
    }
}

/// `text` without invisible characters, in NFKC form.
fn normalise(text: &str) -> String {
    text.chars()
        .filter(|c| !is_invisible(*c))
        .nfkc()
        .filter(|c| !is_invisible(*c))
        .collect()
}

fn mixed_script(token: &str) -> bool {
    let mut seen: Vec<Script> = Vec::new();
    for s in token.chars().filter_map(script) {
        if !seen.contains(&s) {
            seen.push(s);
        }
    }
    seen.len() > 1
}

fn matches_word(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if mixed_script(token) {
        return true;
    }
    let folded: String = token
        .chars()
        .flat_map(char::to_lowercase)
        .map(fold)
        .collect();
    STEMS
        .iter()
        .any(|(a, b)| folded.starts_with(&format!("{a}{b}")))
        || WHOLE.iter().any(|(a, b)| folded == format!("{a}{b}"))
}

/// How many words `text` would lose.
pub fn count(text: &str) -> usize {
    clean(text).1
}

/// `text` normalised with each filtered word replaced, and how many were replaced.
pub fn clean(text: &str) -> (String, usize) {
    let text = normalise(text);
    let (mut out, mut found, mut token) = (String::new(), 0, String::new());
    let mut flush = |token: &mut String, out: &mut String| {
        if matches_word(token) {
            found += 1;
            out.push_str(REPLACEMENT);
        } else {
            out.push_str(token);
        }
        token.clear();
    };
    for c in text.chars() {
        if is_word_char(c) {
            token.push(c);
        } else {
            flush(&mut token, &mut out);
            out.push(c);
        }
    }
    flush(&mut token, &mut out);
    (out, found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(a: &str, b: &str) -> String {
        format!("{a}{b}")
    }

    #[test]
    fn replaces_the_word_and_keeps_the_sentence() {
        let text = format!(
            "Adds a cron job. The change is {}. It runs hourly.",
            make("sa", "fe")
        );
        let (out, found) = clean(&text);
        assert_eq!(
            out,
            format!("Adds a cron job. The change is {REPLACEMENT}. It runs hourly.")
        );
        assert_eq!(found, 1);
    }

    #[test]
    fn stems_and_case_are_matched() {
        for word in ["sa", "se", "sand", "iso", "guar", "pre"] {
            let stem = STEMS.iter().find(|(a, _)| *a == word).unwrap();
            let long = format!("{}{}s", stem.0.to_uppercase(), stem.1.to_uppercase());
            assert_eq!(count(&long), 1, "{long}");
        }
        assert_eq!(
            count(&format!(
                "{}ly and {}",
                make("sa", "fe"),
                make("se", "curity")
            )),
            2
        );
        assert_eq!(
            count(&format!(
                "{} and {}",
                make("con", "tained"),
                make("Con", "Tained")
            )),
            2
        );
    }

    #[test]
    fn allowed_words_stay() {
        for text in [
            "unsafe",
            "insecure",
            "contains",
            "container",
            "contain",
            "containers",
        ] {
            assert_eq!(count(text), 0, "{text}");
        }
        assert_eq!(clean("Runs in a container; it contains a script.").1, 0);
    }

    #[test]
    fn invisible_characters_do_not_hide_a_word() {
        let word = make("sa", "fe");
        let zero_width = format!("{}\u{200B}{}", &word[..2], &word[2..]);
        let soft_hyphen = format!("{}\u{00AD}{}", &word[..1], &word[1..]);
        let tag = format!("\u{E0041}{word}");
        assert_eq!(count(&zero_width), 1);
        assert_eq!(count(&soft_hyphen), 1);
        assert_eq!(count(&tag), 1);
    }

    #[test]
    fn compatibility_forms_are_folded() {
        let fullwidth: String = make("sa", "fe")
            .chars()
            .map(|c| char::from_u32(c as u32 - 0x61 + 0xFF41).unwrap())
            .collect();
        assert_eq!(count(&fullwidth), 1);
        assert_eq!(count("\u{1D5CC}afe"), 1);
    }

    #[test]
    fn look_alike_letters_and_mixed_scripts_match() {
        // Cyrillic s, a, e with a Latin f; and a word that is Cyrillic only except one letter.
        assert_eq!(count("ѕаfе"), 1);
        assert_eq!(count("sаfe"), 1);
        assert_eq!(count("ѕесurе"), 1);
        assert_eq!(count("привет"), 0);
    }
}
