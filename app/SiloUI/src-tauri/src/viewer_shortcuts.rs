//! Pure shortcut policy for native desktop viewer triggers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shortcut {
    PasteIntoComputer,
    CopyFromComputer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Platform {
    Mac,
    Linux,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Keys {
    /// Command on macOS, Super on Linux.
    pub command: bool,
    pub control: bool,
    pub option: bool,
    pub shift: bool,
}

/// Command+C and Command+V on macOS. Ctrl+C and Ctrl+V are ordinary guest
/// shortcuts on Linux, so Silo's triggers there are Ctrl+Shift+C and Ctrl+Shift+V.
/// `letter` is the layout-resolved character, so other keyboard layouts work.
pub(crate) fn classify(platform: Platform, keys: Keys, letter: Option<char>) -> Option<Shortcut> {
    let exact = match platform {
        Platform::Mac => keys.command && !keys.control && !keys.option && !keys.shift,
        Platform::Linux => keys.control && keys.shift && !keys.option && !keys.command,
    };
    if !exact {
        return None;
    }
    match letter?.to_ascii_lowercase() {
        'v' => Some(Shortcut::PasteIntoComputer),
        'c' => Some(Shortcut::CopyFromComputer),
        _ => None,
    }
}

pub(crate) fn clipboard_shortcuts_enabled() -> bool {
    true
}

/// Where keyboard input goes inside a viewer window, as far as the native layer can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) enum Focus {
    /// An editable native text control, such as a field editor.
    EditableText,
    /// The shell's own web content, whose inputs keep ordinary copy and paste.
    Shell,
    /// Anything else, which includes the guest's web content.
    Other,
}

/// Whether a viewer shortcut is taken for the computer rather than left to the focused control.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn takes_shortcut(focus: Focus) -> bool {
    focus == Focus::Other
}

/// What a key press means once the keys that started a shortcut are known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) enum Press {
    /// A new press of a shortcut: start its transfer.
    Start(Shortcut),
    /// Autorepeat of a key that started a shortcut: consumed, whatever modifiers are down now.
    Swallow,
    /// Not Silo's: the focused control receives it.
    Pass,
}

/// Tracks the physical keys that started a shortcut, so holding one down starts one transfer.
#[derive(Debug, Default)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) struct HeldKeys {
    codes: Vec<u16>,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl HeldKeys {
    /// Decides a press of `code`, given what its modifiers classify as. A key already down
    /// from a shortcut stays consumed even after the modifiers were released, so its
    /// autorepeat never reaches the guest while its release is swallowed.
    pub(crate) fn decide(&mut self, code: u16, shortcut: Option<Shortcut>) -> Press {
        if self.codes.contains(&code) {
            return Press::Swallow;
        }
        match shortcut {
            Some(shortcut) => {
                self.codes.push(code);
                Press::Start(shortcut)
            }
            None => Press::Pass,
        }
    }

    /// A release of `code`; true when it ends a tracked press.
    pub(crate) fn release(&mut self, code: u16) -> bool {
        let before = self.codes.len();
        self.codes.retain(|held| *held != code);
        self.codes.len() != before
    }

    /// The window lost focus, so no release will be seen.
    pub(crate) fn clear(&mut self) {
        self.codes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mac(command: bool, control: bool, option: bool, shift: bool) -> Keys {
        Keys {
            command,
            control,
            option,
            shift,
        }
    }

    #[test]
    fn macos_triggers_are_exactly_command_c_and_command_v() {
        let cmd = mac(true, false, false, false);
        assert_eq!(
            classify(Platform::Mac, cmd, Some('v')),
            Some(Shortcut::PasteIntoComputer)
        );
        assert_eq!(
            classify(Platform::Mac, cmd, Some('C')),
            Some(Shortcut::CopyFromComputer)
        );
        assert_eq!(classify(Platform::Mac, cmd, Some('x')), None);
        assert_eq!(classify(Platform::Mac, cmd, None), None);
        for keys in [
            mac(false, false, false, false),
            mac(true, true, false, false),
            mac(true, false, true, false),
            mac(true, false, false, true),
            mac(false, true, false, false),
        ] {
            assert_eq!(classify(Platform::Mac, keys, Some('v')), None, "{keys:?}");
        }
    }

    fn press(held: &mut HeldKeys, code: u16) -> bool {
        matches!(
            held.decide(code, Some(Shortcut::CopyFromComputer)),
            Press::Start(_)
        )
    }

    #[test]
    fn a_held_shortcut_key_starts_one_transfer_until_it_is_released_or_focus_is_lost() {
        let mut held = HeldKeys::default();
        assert!(press(&mut held, 54));
        assert!(!press(&mut held, 54), "autorepeat");
        assert!(!press(&mut held, 54));
        assert!(!held.release(55), "another key");
        assert!(!press(&mut held, 54));
        assert!(held.release(54));
        assert!(!held.release(54));
        assert!(press(&mut held, 54), "a new press after the release");
        held.clear();
        assert!(press(&mut held, 54), "a press after focus returns");
        held.clear();
        // A second shortcut key pressed while the first is down is a new transfer, and the
        // first one's autorepeat stays suppressed.
        assert!(press(&mut held, 54));
        assert!(press(&mut held, 55));
        assert!(!press(&mut held, 54));
    }

    #[test]
    fn autorepeat_stays_consumed_after_the_modifiers_are_released_first() {
        let mut held = HeldKeys::default();
        let paste = Some(Shortcut::PasteIntoComputer);
        assert_eq!(
            held.decide(55, paste),
            Press::Start(Shortcut::PasteIntoComputer)
        );
        // Ctrl and Shift come up before V does: the repeats no longer classify.
        assert_eq!(held.decide(55, None), Press::Swallow);
        assert_eq!(held.decide(55, None), Press::Swallow);
        assert_eq!(held.decide(55, paste), Press::Swallow);
        // The final release is swallowed too, and the next plain V is the guest's.
        assert!(held.release(55));
        assert_eq!(held.decide(55, None), Press::Pass);
        assert_eq!(
            held.decide(55, paste),
            Press::Start(Shortcut::PasteIntoComputer)
        );
        held.clear();
        assert_eq!(
            held.decide(55, None),
            Press::Pass,
            "focus loss forgets the key"
        );
    }

    #[test]
    fn only_the_guest_side_of_a_viewer_window_gives_up_its_shortcuts() {
        assert!(takes_shortcut(Focus::Other));
        assert!(!takes_shortcut(Focus::Shell));
        assert!(!takes_shortcut(Focus::EditableText));
    }

    #[test]
    fn linux_triggers_are_ctrl_shift_c_and_ctrl_shift_v_so_plain_ctrl_v_reaches_the_guest() {
        let chord = mac(false, true, false, true);
        assert_eq!(
            classify(Platform::Linux, chord, Some('V')),
            Some(Shortcut::PasteIntoComputer)
        );
        assert_eq!(
            classify(Platform::Linux, chord, Some('c')),
            Some(Shortcut::CopyFromComputer)
        );
        for keys in [
            mac(false, true, false, false),
            mac(false, true, true, true),
            mac(true, true, false, true),
            mac(false, false, false, true),
        ] {
            assert_eq!(classify(Platform::Linux, keys, Some('v')), None, "{keys:?}");
        }
    }
}
