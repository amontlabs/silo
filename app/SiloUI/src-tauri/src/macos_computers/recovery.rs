//! Turns off System Integrity Protection by driving macOS Recovery.
//!
//! Recovery has no command channel, so Silo plays the keyboard: it starts the
//! computer in Recovery in a visible "Setting up" window, reads the screen by
//! recognizing text in an image of that window, opens Terminal, answers
//! `csrutil disable` and halts the guest. Each step waits for the text that
//! proves the previous one worked instead of sleeping, and falls back to timed
//! waits only when the screen cannot be read at all. The key sequence follows
//! cirruslabs' macos-image-templates and the prompts follow Lume's `sip`
//! command (both MIT).
use super::engine;
use super::input::{self, Keyboard, KeyEvent, Modifier, PointerKind, TextLine};
use super::store::Layout;
use std::time::{Duration, Instant};
use tauri::AppHandle;

const ATTEMPTS: usize = 2;
const POLL: Duration = Duration::from_millis(1500);
const KEY_GAP: Duration = Duration::from_millis(30);
const PICKER_WAIT: Duration = Duration::from_secs(120);
const RECOVERY_WAIT: Duration = Duration::from_secs(150);
const TERMINAL_WAIT: Duration = Duration::from_secs(15);
const PROMPT_WAIT: Duration = Duration::from_secs(45);
const RESULT_WAIT: Duration = Duration::from_secs(90);
const HALT_WAIT: Duration = Duration::from_secs(120);
const STOP_WAIT: Duration = Duration::from_secs(30);
/// Screen reads that fail in a row before the screen counts as unreadable.
const UNREADABLE_AFTER: usize = 5;
/// How much of the last screen an error quotes.
const QUOTE: usize = 300;

/// Turns off System Integrity Protection on the stopped computer `id` and
/// leaves it stopped. The computer's account must exist with the stored password.
pub(super) fn disable_sip(app: &AppHandle, id: &str) -> Result<(), String> {
    let (record, _) = super::computer(id)?;
    let layout = Layout::new(&super::app_data(app)?, id);
    let account = super::guest_access::account(&layout)?;
    let title = format!("Setting up {}", record.name);
    let mut failure = String::new();
    for attempt in 1..=ATTEMPTS {
        engine::start_in_recovery(app, &record, &layout)?;
        let outcome = super::show_display(app, id, &title)
            .and_then(|()| engine::focus_display(app, id).map(|_| ()))
            .map_err(Failure::Fatal)
            .and_then(|()| {
                let mut guest = LiveGuest::new(app, id);
                run(&mut guest, &account.user, &account.password)
            });
        match outcome {
            Ok(()) => return finish(app, id),
            Err(error) => {
                stop(app, id);
                match error {
                    Failure::Retry(message) if attempt < ATTEMPTS => failure = message,
                    Failure::Retry(message) | Failure::Fatal(message) => return Err(message),
                }
            }
        }
    }
    Err(failure)
}

/// Waits for the halted guest to stop, forcing it if it does not.
fn finish(app: &AppHandle, id: &str) -> Result<(), String> {
    if engine::wait_until_stopped(app, id, HALT_WAIT) {
        return Ok(());
    }
    stop(app, id);
    Err("The computer did not shut down after System Integrity Protection was changed.".into())
}

/// Force-stops the computer and waits for it to be gone.
fn stop(app: &AppHandle, id: &str) {
    let _ = engine::force_stop(app, id);
    let _ = engine::wait_until_stopped(app, id, STOP_WAIT);
}

// MARK: Screens

/// The recognized text of one capture of the guest's screen.
struct Screen {
    lines: Vec<TextLine>,
    /// Lower-case text, one line per row, top to bottom.
    text: String,
}

impl Screen {
    fn new(lines: Vec<TextLine>) -> Self {
        let text = lines
            .iter()
            .map(|line| line.text.to_lowercase())
            .collect::<Vec<_>>()
            .join("\n");
        Self { lines, text }
    }

    fn contains(&self, needle: &str) -> bool {
        self.text.contains(needle)
    }

    /// The first line that starts with `prefix`.
    fn line_starting(&self, prefix: &str) -> Option<&TextLine> {
        self.lines
            .iter()
            .find(|line| line.text.to_lowercase().starts_with(prefix))
    }
}

/// What Terminal's `csrutil` is asking or saying, taken from the latest prompt
/// on the screen.
#[derive(Debug, PartialEq, Eq)]
enum Prompt {
    Confirm,
    Username,
    /// A password prompt, with the user it names when it names one.
    Password(Option<String>),
    Done,
    Rejected(String),
}

const DONE: [&str; 3] = [
    "integrity protection is off",
    "successfully disabled system integrity protection",
    "integrity protection is already",
];
const REJECTED: [&str; 6] = [
    "authentication failure",
    "failed to authenticate",
    "failed to set credential",
    "no administrator",
    "incorrect password",
    "failed to modify",
];

fn classify(text: &str) -> Option<Prompt> {
    if DONE.iter().any(|done| text.contains(done)) {
        return Some(Prompt::Done);
    }
    if let Some(reason) = REJECTED.iter().find(|reason| text.contains(**reason)) {
        return Some(Prompt::Rejected((*reason).to_string()));
    }
    // The latest prompt wins: earlier ones stay on screen.
    let latest = [
        ("y/n]", Prompt::Confirm),
        ("authorized user", Prompt::Username),
        ("password", Prompt::Password(password_user(text))),
    ]
    .into_iter()
    .filter_map(|(marker, prompt)| text.rfind(marker).map(|at| (at, prompt)))
    .max_by_key(|(at, _)| *at);
    latest.map(|(_, prompt)| prompt)
}

/// The user in "enter password for user <name>:".
fn password_user(text: &str) -> Option<String> {
    const LEAD: &str = "password for user ";
    let rest = &text[text.rfind(LEAD)? + LEAD.len()..];
    let name: String = rest
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != ':')
        .collect();
    (!name.is_empty()).then_some(name)
}

// MARK: Guests

/// The guest's screen and keyboard.
trait Guest {
    /// The text on screen, or why it cannot be read.
    fn screen(&mut self) -> Result<Vec<TextLine>, String>;
    fn keys(&mut self, events: Vec<KeyEvent>) -> Result<(), String>;
    fn pointer(&mut self, kind: PointerKind, x: f64, y: f64) -> Result<(), String>;
    fn pause(&mut self, duration: Duration);
    fn elapsed(&self) -> Duration;
}

struct LiveGuest<'a> {
    app: &'a AppHandle,
    id: &'a str,
    started: Instant,
}

impl<'a> LiveGuest<'a> {
    fn new(app: &'a AppHandle, id: &'a str) -> Self {
        Self {
            app,
            id,
            started: Instant::now(),
        }
    }
}

impl Guest for LiveGuest<'_> {
    fn screen(&mut self) -> Result<Vec<TextLine>, String> {
        engine::read_screen(self.app, self.id)
    }

    fn keys(&mut self, events: Vec<KeyEvent>) -> Result<(), String> {
        engine::send_keys(self.app, self.id, events)
    }

    fn pointer(&mut self, kind: PointerKind, x: f64, y: f64) -> Result<(), String> {
        engine::send_pointer(self.app, self.id, kind, x, y)
    }

    fn pause(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

// MARK: Driving

/// Why a run ended without disabling SIP. A retry starts Recovery over; it
/// helps when navigation went wrong and never once credentials were typed.
#[derive(Debug)]
enum Failure {
    Retry(String),
    Fatal(String),
}

impl Failure {
    fn fatal(self) -> Self {
        match self {
            Self::Retry(message) | Self::Fatal(message) => Self::Fatal(message),
        }
    }
}

enum Check<T> {
    Found(T),
    Press(u16),
    Rejected(String),
    Waiting,
}

struct Driver<'a, G: Guest> {
    guest: &'a mut G,
    keyboard: Keyboard,
    unreadable: usize,
    last: String,
}

impl<G: Guest> Driver<'_, G> {
    fn blind(&self) -> bool {
        self.unreadable >= UNREADABLE_AFTER
    }

    fn send(&mut self, events: Vec<KeyEvent>) -> Result<(), Failure> {
        self.guest.keys(events).map_err(|message| {
            Failure::Fatal(format!(
                "Silo lost the computer's setup window. {message}"
            ))
        })
    }

    fn press(&mut self, code: u16) -> Result<(), Failure> {
        let events = self.keyboard.tap(code, None).to_vec();
        self.send(events)
    }

    fn type_text(&mut self, text: &str) -> Result<(), Failure> {
        for c in text.chars() {
            let events = self.keyboard.character(c).map_err(Failure::Fatal)?;
            self.send(events)?;
            self.guest.pause(KEY_GAP);
        }
        Ok(())
    }

    fn type_line(&mut self, text: &str) -> Result<(), Failure> {
        self.type_text(text)?;
        self.press(input::RETURN)
    }

    fn click(&mut self, line: &TextLine) -> Result<(), Failure> {
        let (x, y) = line.center();
        for (kind, gap) in [
            (PointerKind::Move, 300),
            (PointerKind::Down, 100),
            (PointerKind::Up, 0),
        ] {
            self.guest
                .pointer(kind, x, y)
                .map_err(|message| Failure::Fatal(format!("Silo lost the setup window. {message}")))?;
            self.guest.pause(Duration::from_millis(gap));
        }
        Ok(())
    }

    /// One reading of the screen; `None` when the screen cannot be read.
    fn look(&mut self) -> Option<Screen> {
        match self.guest.screen() {
            Ok(lines) => {
                self.unreadable = 0;
                let screen = Screen::new(lines);
                self.last = screen.text.replace('\n', " | ");
                Some(screen)
            }
            Err(_) => {
                self.unreadable += 1;
                None
            }
        }
    }

    fn quote(&self) -> String {
        if self.last.is_empty() {
            return "The screen could not be read.".into();
        }
        let shown: String = self.last.chars().take(QUOTE).collect();
        format!("It showed: {shown}")
    }

    /// Polls the screen until `check` finds what it waits for. `None` means the
    /// screen is unreadable and `blind` was waited out instead.
    fn wait_for<T>(
        &mut self,
        what: &str,
        timeout: Duration,
        blind: Duration,
        mut check: impl FnMut(&Screen) -> Check<T>,
    ) -> Result<Option<T>, Failure> {
        let deadline = self.guest.elapsed() + timeout;
        loop {
            if self.blind() {
                self.guest.pause(blind);
                return Ok(None);
            }
            if let Some(screen) = self.look() {
                match check(&screen) {
                    Check::Found(found) => return Ok(Some(found)),
                    Check::Press(code) => {
                        self.press(code)?;
                        self.guest.pause(Duration::from_secs(4));
                    }
                    Check::Rejected(reason) => {
                        return Err(Failure::Fatal(format!(
                            "Recovery refused the account: {reason}. {}",
                            self.quote()
                        )))
                    }
                    Check::Waiting => {}
                }
            }
            if self.guest.elapsed() >= deadline {
                return Err(Failure::Retry(format!(
                    "Recovery did not show {what}. {}",
                    self.quote()
                )));
            }
            self.guest.pause(POLL);
        }
    }
}

/// Plays the whole sequence on a guest that has just been started in Recovery,
/// and returns once the halt command is sent.
fn run<G: Guest>(guest: &mut G, user: &str, password: &str) -> Result<(), Failure> {
    let mut driver = Driver {
        guest,
        keyboard: Keyboard::default(),
        unreadable: 0,
        last: String::new(),
    };
    pick_options(&mut driver)?;
    open_terminal(&mut driver)?;
    driver.type_line("clear")?;
    driver.guest.pause(Duration::from_secs(1));
    driver.type_line("csrutil disable")?;
    answer_prompts(&mut driver, user, password).map_err(Failure::fatal)?;
    driver.guest.pause(Duration::from_secs(2));
    driver.type_line("halt").map_err(Failure::fatal)
}

/// Gets from the startup picker to Recovery's main window.
fn pick_options<G: Guest>(driver: &mut Driver<'_, G>) -> Result<(), Failure> {
    driver.wait_for(
        "its startup options",
        PICKER_WAIT,
        Duration::from_secs(30),
        |screen| {
            if screen.contains("options") {
                Check::Found(())
            } else {
                Check::Waiting
            }
        },
    )?;
    // Skip "Macintosh HD" to reach "Options".
    for _ in 0..2 {
        driver.press(input::RIGHT)?;
        driver.guest.pause(Duration::from_secs(1));
    }
    driver.press(input::RETURN)?;
    driver.wait_for(
        "its main window",
        RECOVERY_WAIT,
        Duration::from_secs(45),
        |screen| {
            if screen.contains("utilities") {
                Check::Found(())
            } else if screen.contains("language") {
                Check::Press(input::RETURN)
            } else {
                Check::Waiting
            }
        },
    )?;
    Ok(())
}

/// Opens Terminal with its shortcut, or from the Utilities menu when the
/// shortcut does nothing.
fn open_terminal<G: Guest>(driver: &mut Driver<'_, G>) -> Result<(), Failure> {
    let shortcut = driver
        .keyboard
        .chord(&[Modifier::Shift, Modifier::Command], input::KEY_T, Some('t'));
    driver.send(shortcut)?;
    let shown = |screen: &Screen| {
        if screen.contains("bash") {
            Check::Found(())
        } else {
            Check::Waiting
        }
    };
    match driver.wait_for("Terminal", Duration::from_secs(8), Duration::from_secs(8), shown) {
        Ok(_) => return Ok(()),
        Err(Failure::Retry(_)) => {}
        Err(other) => return Err(other),
    }
    let menu = driver
        .look()
        .and_then(|screen| screen.line_starting("utilities").cloned())
        .ok_or_else(|| Failure::Retry(format!("Recovery has no Utilities menu. {}", driver.quote())))?;
    driver.click(&menu)?;
    driver.guest.pause(Duration::from_secs(1));
    let item = driver
        .look()
        .and_then(|screen| screen.line_starting("terminal").cloned())
        .ok_or_else(|| Failure::Retry(format!("The Utilities menu has no Terminal. {}", driver.quote())))?;
    driver.click(&item)?;
    driver.wait_for("Terminal", TERMINAL_WAIT, Duration::from_secs(8), shown)?;
    Ok(())
}

/// Answers `csrutil`'s questions until it reports the result. macOS 26 asks for
/// a user name before the password; earlier versions name the user in the
/// password prompt.
fn answer_prompts<G: Guest>(
    driver: &mut Driver<'_, G>,
    user: &str,
    password: &str,
) -> Result<(), Failure> {
    let (mut confirmed, mut named, mut authenticated) = (false, false, false);
    if driver.blind() {
        driver.guest.pause(Duration::from_secs(5));
        driver.type_line("y")?;
        driver.guest.pause(Duration::from_secs(5));
        driver.type_line(user)?;
        driver.guest.pause(Duration::from_secs(5));
        driver.type_line(password)?;
        driver.guest.pause(Duration::from_secs(20));
        return Ok(());
    }
    loop {
        let wait = if authenticated { RESULT_WAIT } else { PROMPT_WAIT };
        let prompt = driver.wait_for("a csrutil prompt", wait, Duration::from_secs(5), |screen| {
            match classify(&screen.text) {
                Some(Prompt::Done) => Check::Found(Prompt::Done),
                Some(Prompt::Rejected(reason)) => Check::Rejected(reason),
                Some(Prompt::Confirm) if !confirmed => Check::Found(Prompt::Confirm),
                Some(Prompt::Username) if !named => Check::Found(Prompt::Username),
                Some(Prompt::Password(who)) if !authenticated => {
                    Check::Found(Prompt::Password(who))
                }
                _ => Check::Waiting,
            }
        })?;
        match prompt {
            Some(Prompt::Done) => return Ok(()),
            Some(Prompt::Confirm) => {
                confirmed = true;
                driver.type_line("y")?;
            }
            Some(Prompt::Username) => {
                named = true;
                driver.type_line(user)?;
            }
            Some(Prompt::Password(who)) => {
                if !named && who.as_deref().is_some_and(|who| who.starts_with('_')) {
                    return Err(Failure::Fatal(format!(
                        "Recovery asked for the password of {}, not of {user}, so it does not know the account. {}",
                        who.unwrap_or_default(),
                        driver.quote()
                    )));
                }
                authenticated = true;
                driver.type_line(password)?;
            }
            Some(Prompt::Rejected(_)) | None => return Ok(()),
        }
        driver.guest.pause(Duration::from_secs(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, x: f64, y: f64) -> TextLine {
        TextLine {
            text: text.into(),
            x,
            y,
            width: 0.1,
            height: 0.02,
        }
    }

    #[test]
    fn the_latest_prompt_wins() {
        let confirm = "turning off system integrity protection requires modifying system security.\nallow booting unsigned operating systems and any kernel extensions for os \"macintosh hd\"? [y/n]:";
        assert_eq!(classify(confirm), Some(Prompt::Confirm));
        let user = format!("{confirm} y\nenter a username of an authorized user:");
        assert_eq!(classify(&user), Some(Prompt::Username));
        let password = format!("{user} silo\npassword:");
        assert_eq!(classify(&password), Some(Prompt::Password(None)));
    }

    #[test]
    fn a_password_prompt_names_its_user() {
        let text = "[y/n]: y\nenter password for user _mbsetupuser:";
        assert_eq!(
            classify(text),
            Some(Prompt::Password(Some("_mbsetupuser".into())))
        );
    }

    #[test]
    fn results_override_prompts() {
        assert_eq!(
            classify("[y/n]: y\npassword:\nsystem integrity protection is off."),
            Some(Prompt::Done)
        );
        assert!(matches!(
            classify("password for user silo:\ncsrutil: failed to set credential: authentication failure."),
            Some(Prompt::Rejected(_))
        ));
        assert_eq!(classify("-bash-3.2#"), None);
    }

    #[test]
    fn a_line_is_found_by_its_start() {
        let screen = Screen::new(vec![line("Recovery", 0.1, 0.9), line("Utilities", 0.3, 0.9)]);
        assert_eq!(screen.line_starting("utilities").unwrap().x, 0.3);
        assert!(screen.line_starting("terminal").is_none());
    }

    /// A scripted Recovery: it reacts to the keys and clicks it receives and to
    /// the time that passes.
    struct Recovery {
        now: Duration,
        phase: Phase,
        entered: Duration,
        typed: String,
        picked: usize,
        shortcut_works: bool,
        username_prompt: bool,
        readable: bool,
        password_ok: bool,
        menu_open: bool,
        keys: Vec<String>,
    }

    #[derive(PartialEq, Clone, Copy, Debug)]
    enum Phase {
        Picker,
        Booting,
        Main,
        Terminal,
        Confirm,
        Username,
        Password,
        Result,
        Rejected,
        Halted,
    }

    impl Recovery {
        fn new() -> Self {
            Self {
                now: Duration::ZERO,
                phase: Phase::Picker,
                entered: Duration::ZERO,
                typed: String::new(),
                picked: 0,
                shortcut_works: true,
                username_prompt: true,
                readable: true,
                password_ok: true,
                menu_open: false,
                keys: Vec::new(),
            }
        }

        fn enter(&mut self, phase: Phase) {
            self.phase = phase;
            self.entered = self.now;
            self.typed.clear();
        }

        fn submit(&mut self) {
            let typed = std::mem::take(&mut self.typed);
            self.keys.push(format!("line:{typed}"));
            match (self.phase, typed.as_str()) {
                (Phase::Terminal, "csrutil disable") => self.enter(Phase::Confirm),
                (Phase::Confirm, "y") => self.enter(if self.username_prompt {
                    Phase::Username
                } else {
                    Phase::Password
                }),
                (Phase::Username, "silo") => self.enter(Phase::Password),
                (Phase::Password, "secret") if self.password_ok => self.enter(Phase::Result),
                (Phase::Password, _) => self.enter(Phase::Rejected),
                (Phase::Result, "halt") => self.enter(Phase::Halted),
                _ => {}
            }
        }
    }

    impl Guest for Recovery {
        fn screen(&mut self) -> Result<Vec<TextLine>, String> {
            if !self.readable {
                return Err("no capture".into());
            }
            let rows: Vec<&str> = match self.phase {
                Phase::Picker if self.now >= self.entered + Duration::from_secs(12) => {
                    vec!["Macintosh HD", "Options", "Shut Down"]
                }
                Phase::Main => {
                    if self.menu_open {
                        vec!["Recovery", "Utilities", "Startup Security Utility", "Terminal ⇧⌘T"]
                    } else {
                        vec!["Recovery", "File", "Utilities", "Restore from Time Machine"]
                    }
                }
                Phase::Terminal => vec!["Terminal", "-bash-3.2#"],
                Phase::Confirm => vec![
                    "-bash-3.2# csrutil disable",
                    "Allow booting unsigned operating systems? [y/n]:",
                ],
                Phase::Username => vec!["[y/n]: y", "Enter a username of an authorized user:"],
                Phase::Password if self.username_prompt => vec!["authorized user: silo", "Password:"],
                Phase::Password => vec!["[y/n]: y", "Enter password for user silo:"],
                Phase::Result => vec!["System Integrity Protection is off."],
                Phase::Rejected => vec![
                    "csrutil: failed to set credential: Authentication failure.",
                    "-bash-3.2#",
                ],
                _ => vec![],
            };
            Ok(rows
                .iter()
                .enumerate()
                .map(|(index, text)| line(text, 0.1, 0.9 - index as f64 * 0.05))
                .collect())
        }

        fn keys(&mut self, events: Vec<KeyEvent>) -> Result<(), String> {
            for event in events {
                if event.kind == input::EventKind::KeyDown {
                    match (self.phase, event.code) {
                        (Phase::Picker, input::RIGHT) => self.picked += 1,
                        (Phase::Picker, input::RETURN) => {
                            let phase = if self.picked == 2 {
                                Phase::Booting
                            } else {
                                Phase::Halted
                            };
                            self.enter(phase);
                        }
                        (Phase::Main, input::KEY_T) if self.shortcut_works => {
                            self.keys.push("shortcut".into());
                            self.enter(Phase::Terminal);
                        }
                        (_, input::RETURN) => self.submit(),
                        _ => {
                            if let Some(c) = event.characters.chars().next() {
                                self.typed.push(c);
                            }
                        }
                    }
                }
            }
            Ok(())
        }

        fn pointer(&mut self, kind: PointerKind, _x: f64, y: f64) -> Result<(), String> {
            if kind != PointerKind::Down || self.phase != Phase::Main {
                return Ok(());
            }
            if !self.menu_open && (y - 0.81).abs() < 0.02 {
                self.menu_open = true;
            } else if self.menu_open && (y - 0.76).abs() < 0.02 {
                self.keys.push("menu".into());
                self.enter(Phase::Terminal);
            }
            Ok(())
        }

        fn pause(&mut self, duration: Duration) {
            self.now += duration;
            if self.phase == Phase::Booting && self.now >= self.entered + Duration::from_secs(6) {
                self.phase = Phase::Main;
            }
        }

        fn elapsed(&self) -> Duration {
            self.now
        }
    }

    fn lines_typed(guest: &Recovery) -> Vec<String> {
        guest
            .keys
            .iter()
            .filter_map(|key| key.strip_prefix("line:").map(str::to_string))
            .collect()
    }

    #[test]
    fn the_modern_flow_asks_for_a_user_then_a_password() {
        let mut guest = Recovery::new();
        run(&mut guest, "silo", "secret").unwrap();
        assert_eq!(guest.phase, Phase::Halted);
        assert_eq!(
            lines_typed(&guest),
            ["clear", "csrutil disable", "y", "silo", "secret", "halt"]
        );
        assert!(guest.keys.contains(&"shortcut".to_string()));
    }

    #[test]
    fn the_older_flow_asks_for_the_password_directly() {
        let mut guest = Recovery::new();
        guest.username_prompt = false;
        run(&mut guest, "silo", "secret").unwrap();
        assert_eq!(guest.phase, Phase::Halted);
        assert_eq!(
            lines_typed(&guest),
            ["clear", "csrutil disable", "y", "secret", "halt"]
        );
    }

    #[test]
    fn terminal_is_opened_from_the_menu_when_the_shortcut_does_nothing() {
        let mut guest = Recovery::new();
        guest.shortcut_works = false;
        run(&mut guest, "silo", "secret").unwrap();
        assert!(guest.keys.contains(&"menu".to_string()));
        assert_eq!(guest.phase, Phase::Halted);
    }

    #[test]
    fn a_rejected_password_fails_without_retrying() {
        let mut guest = Recovery::new();
        guest.password_ok = false;
        let error = run(&mut guest, "silo", "secret").unwrap_err();
        match error {
            Failure::Fatal(message) => {
                assert!(message.contains("authentication failure"), "{message}");
                assert!(!message.contains("secret"));
            }
            Failure::Retry(message) => panic!("retryable: {message}"),
        }
        assert_ne!(guest.phase, Phase::Halted);
    }

    #[test]
    fn recovery_that_never_appears_is_a_retryable_failure() {
        struct Dead(Recovery);
        impl Guest for Dead {
            fn screen(&mut self) -> Result<Vec<TextLine>, String> {
                Ok(vec![line("Apple logo", 0.5, 0.5)])
            }
            fn keys(&mut self, events: Vec<KeyEvent>) -> Result<(), String> {
                self.0.keys(events)
            }
            fn pointer(&mut self, kind: PointerKind, x: f64, y: f64) -> Result<(), String> {
                self.0.pointer(kind, x, y)
            }
            fn pause(&mut self, duration: Duration) {
                self.0.pause(duration);
            }
            fn elapsed(&self) -> Duration {
                self.0.elapsed()
            }
        }
        let mut guest = Dead(Recovery::new());
        let error = run(&mut guest, "silo", "secret").unwrap_err();
        assert!(matches!(error, Failure::Retry(message) if message.contains("apple logo")));
        assert!(guest.0.now >= PICKER_WAIT);
    }

    #[test]
    fn an_unreadable_screen_falls_back_to_timed_waits() {
        let mut guest = Recovery::new();
        guest.readable = false;
        run(&mut guest, "silo", "secret").unwrap();
        assert_eq!(guest.phase, Phase::Halted);
    }

    #[test]
    fn a_lost_window_is_fatal() {
        struct Closed;
        impl Guest for Closed {
            fn screen(&mut self) -> Result<Vec<TextLine>, String> {
                Ok(vec![line("Options", 0.5, 0.5)])
            }
            fn keys(&mut self, _: Vec<KeyEvent>) -> Result<(), String> {
                Err("closed".into())
            }
            fn pointer(&mut self, _: PointerKind, _: f64, _: f64) -> Result<(), String> {
                Err("closed".into())
            }
            fn pause(&mut self, _: Duration) {}
            fn elapsed(&self) -> Duration {
                Duration::ZERO
            }
        }
        assert!(matches!(
            run(&mut Closed, "silo", "secret"),
            Err(Failure::Fatal(_))
        ));
    }
}
