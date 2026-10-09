//! Keyboard and pointer input for a macOS guest's display, as plain data.
//!
//! The guest sees the host's virtual key codes, not characters: key codes are
//! physical positions, and the guest's layout (US in Recovery) turns them into
//! characters. Typing text therefore means looking up the US position of each
//! character, whatever layout the host keyboard has.
#![cfg_attr(
    not(all(target_os = "macos", target_arch = "aarch64")),
    allow(dead_code)
)]

pub(super) const RETURN: u16 = 36;
pub(super) const RIGHT: u16 = 124;
pub(super) const DOWN: u16 = 125;

/// `T`'s US position, for the Terminal shortcut.
pub(super) const KEY_T: u16 = 17;

const MASK_SHIFT: usize = 1 << 17;
const MASK_COMMAND: usize = 1 << 20;

/// The device-dependent bits of the left modifier keys. The display view only
/// recognizes a modifier when its device bit is set next to the aggregate one.
const DEVICE_SHIFT: usize = 0x2;
const DEVICE_COMMAND: usize = 0x8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Modifier {
    Shift,
    Command,
}

impl Modifier {
    pub(super) fn key_code(self) -> u16 {
        match self {
            Self::Command => 55,
            Self::Shift => 56,
        }
    }

    fn mask(self) -> usize {
        match self {
            Self::Shift => MASK_SHIFT | DEVICE_SHIFT,
            Self::Command => MASK_COMMAND | DEVICE_COMMAND,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EventKind {
    KeyDown,
    KeyUp,
    FlagsChanged,
}

/// One keyboard event to hand to the display view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct KeyEvent {
    pub kind: EventKind,
    pub code: u16,
    pub characters: String,
    pub flags: usize,
}

/// A character's physical key and whether Shift is held to produce it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Position {
    pub code: u16,
    pub shift: bool,
}

/// The US key code of an unshifted character.
fn base_code(c: char) -> Option<u16> {
    Some(match c {
        'a' => 0,
        's' => 1,
        'd' => 2,
        'f' => 3,
        'h' => 4,
        'g' => 5,
        'z' => 6,
        'x' => 7,
        'c' => 8,
        'v' => 9,
        'b' => 11,
        'q' => 12,
        'w' => 13,
        'e' => 14,
        'r' => 15,
        'y' => 16,
        't' => 17,
        '1' => 18,
        '2' => 19,
        '3' => 20,
        '4' => 21,
        '6' => 22,
        '5' => 23,
        '=' => 24,
        '9' => 25,
        '7' => 26,
        '-' => 27,
        '8' => 28,
        '0' => 29,
        ']' => 30,
        'o' => 31,
        'u' => 32,
        '[' => 33,
        'i' => 34,
        'p' => 35,
        'l' => 37,
        'j' => 38,
        '\'' => 39,
        'k' => 40,
        ';' => 41,
        '\\' => 42,
        ',' => 43,
        '/' => 44,
        'n' => 45,
        'm' => 46,
        '.' => 47,
        ' ' => 49,
        '`' => 50,
        _ => return None,
    })
}

/// The unshifted character on the same key as a US shifted symbol.
fn unshifted(c: char) -> Option<char> {
    Some(match c {
        '!' => '1',
        '@' => '2',
        '#' => '3',
        '$' => '4',
        '%' => '5',
        '^' => '6',
        '&' => '7',
        '*' => '8',
        '(' => '9',
        ')' => '0',
        '_' => '-',
        '+' => '=',
        '{' => '[',
        '}' => ']',
        '|' => '\\',
        ':' => ';',
        '"' => '\'',
        '<' => ',',
        '>' => '.',
        '?' => '/',
        '~' => '`',
        _ => return None,
    })
}

pub(super) fn position(c: char) -> Option<Position> {
    if let Some(code) = base_code(c) {
        return Some(Position { code, shift: false });
    }
    if c.is_ascii_uppercase() {
        return base_code(c.to_ascii_lowercase()).map(|code| Position { code, shift: true });
    }
    unshifted(c)
        .and_then(base_code)
        .map(|code| Position { code, shift: true })
}

/// Tracks which modifiers are held, so every event carries the flags the
/// guest should see at that moment.
#[derive(Default)]
pub(super) struct Keyboard {
    flags: usize,
}

impl Keyboard {
    pub(super) fn modifier(&mut self, modifier: Modifier, down: bool) -> KeyEvent {
        if down {
            self.flags |= modifier.mask();
        } else {
            self.flags &= !modifier.mask();
        }
        KeyEvent {
            kind: EventKind::FlagsChanged,
            code: modifier.key_code(),
            characters: String::new(),
            flags: self.flags,
        }
    }

    /// A key pressed and released with whatever modifiers are held now.
    pub(super) fn tap(&mut self, code: u16, character: Option<char>) -> [KeyEvent; 2] {
        let characters = character.map(String::from).unwrap_or_default();
        let event = |kind| KeyEvent {
            kind,
            code,
            characters: characters.clone(),
            flags: self.flags,
        };
        [event(EventKind::KeyDown), event(EventKind::KeyUp)]
    }

    /// One character typed with Shift held as its position requires.
    pub(super) fn character(&mut self, c: char) -> Result<Vec<KeyEvent>, String> {
        let position = position(c).ok_or_else(|| format!("Cannot type {c:?} into a guest."))?;
        let mut events = Vec::with_capacity(4);
        if position.shift {
            events.push(self.modifier(Modifier::Shift, true));
        }
        events.extend(self.tap(position.code, Some(c)));
        if position.shift {
            events.push(self.modifier(Modifier::Shift, false));
        }
        Ok(events)
    }

    /// Modifiers held around one key, as in `Shift+Command+T`.
    pub(super) fn chord(
        &mut self,
        modifiers: &[Modifier],
        code: u16,
        character: Option<char>,
    ) -> Vec<KeyEvent> {
        let mut events: Vec<KeyEvent> = modifiers
            .iter()
            .map(|modifier| self.modifier(*modifier, true))
            .collect();
        events.extend(self.tap(code, character));
        events.extend(
            modifiers
                .iter()
                .rev()
                .map(|modifier| self.modifier(*modifier, false)),
        );
        events
    }
}

/// A recognized line of text on the guest's screen. The box is normalized to
/// the captured window, with its origin at the bottom left.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct TextLine {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl TextLine {
    pub(super) fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// `NSEventType` raw values of the keyboard events.
const KEYBOARD_EVENTS: [usize; 3] = [10, 11, 12];

/// `NSEventType` raw values of every event that moves, presses or gestures with
/// a pointing device: mouse (left, right, other) down, up, moved, dragged,
/// entered, exited and cancelled; scroll wheel; tablet point and proximity;
/// rotate, begin and end gesture, gesture, magnify, swipe, smart magnify, quick
/// look; pressure; direct touch; cursor updates and mode changes.
const POINTER_EVENTS: [usize; 28] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 18, 19, 20, 22, 23, 24, 25, 26, 27, 29, 30, 31, 32, 33, 34, 37, 40,
    17, 38,
];

pub(super) fn is_keyboard_event(kind: usize) -> bool {
    KEYBOARD_EVENTS.contains(&kind)
}

pub(super) fn is_pointer_event(kind: usize) -> bool {
    POINTER_EVENTS.contains(&kind)
}

/// Something installed per computer, such as an event monitor. Replacing or
/// removing one computer's entry never touches another's.
pub(super) struct Monitors<T> {
    by_computer: std::collections::HashMap<String, T>,
}

impl<T> Default for Monitors<T> {
    fn default() -> Self {
        Self {
            by_computer: std::collections::HashMap::new(),
        }
    }
}

impl<T> Monitors<T> {
    /// Stores the entry and returns the one it replaces for the same computer.
    pub(super) fn insert(&mut self, id: &str, entry: T) -> Option<T> {
        self.by_computer.insert(id.to_string(), entry)
    }

    pub(super) fn remove(&mut self, id: &str) -> Option<T> {
        self.by_computer.remove(id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PointerKind {
    Move,
    Down,
    Up,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_digits_and_symbols_map_to_us_positions() {
        assert_eq!(
            position('a'),
            Some(Position {
                code: 0,
                shift: false
            })
        );
        assert_eq!(
            position('A'),
            Some(Position {
                code: 0,
                shift: true
            })
        );
        assert_eq!(
            position('q'),
            Some(Position {
                code: 12,
                shift: false
            })
        );
        assert_eq!(
            position('0'),
            Some(Position {
                code: 29,
                shift: false
            })
        );
        assert_eq!(
            position('_'),
            Some(Position {
                code: 27,
                shift: true
            })
        );
        assert_eq!(
            position('-'),
            Some(Position {
                code: 27,
                shift: false
            })
        );
        assert_eq!(
            position('/'),
            Some(Position {
                code: 44,
                shift: false
            })
        );
        assert_eq!(
            position('?'),
            Some(Position {
                code: 44,
                shift: true
            })
        );
        assert_eq!(
            position(' '),
            Some(Position {
                code: 49,
                shift: false
            })
        );
        assert_eq!(position('é'), None);
    }

    #[test]
    fn every_printable_ascii_character_has_a_position() {
        for c in (0x20u8..0x7f).map(char::from) {
            assert!(position(c).is_some(), "{c:?}");
        }
    }

    #[test]
    fn distinct_characters_never_share_a_position() {
        let mut seen = std::collections::HashMap::new();
        for c in (0x20u8..0x7f).map(char::from) {
            let position = position(c).unwrap();
            let key = (position.code, position.shift);
            assert_eq!(seen.insert(key, c), None, "{c:?} collides");
        }
    }

    #[test]
    fn a_shifted_character_is_wrapped_in_shift_events() {
        let mut keyboard = Keyboard::default();
        let events = keyboard.character('T').unwrap();
        let kinds: Vec<_> = events.iter().map(|event| event.kind).collect();
        assert_eq!(
            kinds,
            [
                EventKind::FlagsChanged,
                EventKind::KeyDown,
                EventKind::KeyUp,
                EventKind::FlagsChanged
            ]
        );
        assert_eq!(events[0].code, 56);
        assert_eq!(events[0].flags, MASK_SHIFT | DEVICE_SHIFT);
        assert_eq!(events[1].flags, MASK_SHIFT | DEVICE_SHIFT);
        assert_eq!(events[1].characters, "T");
        assert_eq!(events[3].flags, 0);
    }

    #[test]
    fn an_unshifted_character_is_a_bare_press() {
        let mut keyboard = Keyboard::default();
        let events = keyboard.character('t').unwrap();
        assert_eq!(events.len(), 2);
        assert!(events
            .iter()
            .all(|event| event.flags == 0 && event.code == KEY_T));
    }

    #[test]
    fn a_chord_holds_modifiers_in_order_and_releases_them_in_reverse() {
        let mut keyboard = Keyboard::default();
        let events = keyboard.chord(&[Modifier::Shift, Modifier::Command], KEY_T, Some('t'));
        let codes: Vec<_> = events.iter().map(|event| event.code).collect();
        assert_eq!(codes, [56, 55, KEY_T, KEY_T, 55, 56]);
        let both = MASK_SHIFT | DEVICE_SHIFT | MASK_COMMAND | DEVICE_COMMAND;
        assert_eq!(events[2].flags, both);
        assert_eq!(events[4].flags, MASK_SHIFT | DEVICE_SHIFT);
        assert_eq!(events[5].flags, 0);
    }

    #[test]
    fn typing_text_leaves_no_modifier_held() {
        let mut keyboard = Keyboard::default();
        for c in "Ab_1 !".chars() {
            keyboard.character(c).unwrap();
        }
        assert_eq!(keyboard.flags, 0);
    }

    #[test]
    fn an_untypeable_character_is_refused() {
        assert!(Keyboard::default().character('ü').is_err());
    }

    #[test]
    fn keyboard_and_pointer_events_are_told_apart_by_type() {
        for kind in [10, 11, 12] {
            assert!(is_keyboard_event(kind) && !is_pointer_event(kind), "{kind}");
        }
        // Mouse down/up/moved/dragged/entered/exited for every button, scroll,
        // tablet, gestures, pressure, direct touch and a cancelled mouse.
        for kind in [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 18, 19, 20, 22, 23, 24, 25, 26, 27, 29, 30, 31, 32, 33, 34,
            37, 40,
        ] {
            assert!(is_pointer_event(kind) && !is_keyboard_event(kind), "{kind}");
        }
        // Events that are neither: app-kit, system and application defined, periodic.
        for kind in [0, 13, 14, 15, 16] {
            assert!(
                !is_pointer_event(kind) && !is_keyboard_event(kind),
                "{kind}"
            );
        }
    }

    #[test]
    fn monitors_are_kept_per_computer() {
        let mut monitors = Monitors::default();
        assert_eq!(monitors.insert("a", 1), None);
        assert_eq!(monitors.insert("b", 2), None);
        assert_eq!(monitors.insert("a", 3), Some(1));
        assert_eq!(monitors.remove("a"), Some(3));
        assert_eq!(monitors.remove("a"), None);
        assert_eq!(monitors.remove("b"), Some(2));
    }

    #[test]
    fn a_line_is_clicked_at_its_center() {
        let line = TextLine {
            text: "Utilities".into(),
            x: 0.2,
            y: 0.8,
            width: 0.1,
            height: 0.05,
        };
        let (x, y) = line.center();
        assert!((x - 0.25).abs() < 1e-9 && (y - 0.825).abs() < 1e-9);
    }
}
