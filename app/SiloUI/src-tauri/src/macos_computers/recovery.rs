//! Turns off System Integrity Protection by driving macOS Recovery.
//!
//! Recovery has no command channel, so Silo plays the keyboard: it starts the
//! computer in Recovery in a visible "Setting up" window, reads the screen by
//! recognizing text in an image of the machine's display, opens Terminal,
//! answers `csrutil disable` and halts the guest. Every step waits for the text
//! that proves the previous one worked. When the screen cannot be read, nothing
//! more is typed and the setup stops. The key sequence follows cirruslabs'
//! macos-image-templates and the prompts follow Lume's `sip` command (both MIT).
//!
//! While the sequence runs, the window drops the user's own keyboard and
//! pointer input over the display and says so in its title bar, so stray input
//! cannot reach the guest. Credentials are typed only when the last line of the
//! Terminal is a prompt that names the computer's account.
use super::engine;
use super::guest_access::CANCELLED;
use super::input::{self, KeyEvent, Keyboard, Modifier, PointerKind, TextLine};
use super::store::Layout;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const ATTEMPTS: usize = 2;
const POLL: Duration = Duration::from_millis(1500);
const KEY_GAP: Duration = Duration::from_millis(30);
/// The longest a pause runs before the cancellation flag is looked at again.
const CANCEL_SLICE: Duration = Duration::from_millis(250);
const PICKER_WAIT: Duration = Duration::from_secs(240);
const RECOVERY_WAIT: Duration = Duration::from_secs(240);
const SHORTCUT_WAIT: Duration = Duration::from_secs(8);
const TERMINAL_WAIT: Duration = Duration::from_secs(15);
const PROMPT_WAIT: Duration = Duration::from_secs(45);
const RESULT_WAIT: Duration = Duration::from_secs(90);
const HALT_WAIT: Duration = Duration::from_secs(120);
const STOP_WAIT: Duration = Duration::from_secs(30);
/// Screen reads that fail in a row before the setup gives up.
const UNREADABLE_AFTER: usize = 5;
const LOCKED_SUBTITLE: &str = "Silo is setting up this computer. Please don't type.";

/// Turns off System Integrity Protection on the stopped computer `id` and
/// leaves it stopped. The computer's account must exist with the stored
/// password. `cancelled` is polled between steps; once it is true the computer
/// is force-stopped and the error is `guest_access::CANCELLED`.
///
/// Recovery runs in the language and keyboard layout stored in the computer's
/// NVRAM. `set_language` boots the computer normally and stores English with a
/// US layout; it runs first unless `language_ready`, and again when Recovery
/// still came up in another language.
pub(super) fn disable_sip(
    app: &AppHandle,
    id: &str,
    cancelled: &dyn Fn() -> bool,
    language_ready: bool,
    set_language: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    let (record, _) = super::computer(id)?;
    let layout = Layout::new(&super::app_data(app)?, id);
    let account = super::guest_access::account(&layout)?;
    let title = format!("Setting up {}", record.name);
    let mut failure = String::new();
    if !language_ready {
        set_language()?;
    }
    for attempt in 1..=ATTEMPTS {
        if cancelled() {
            return Err(CANCELLED.into());
        }
        engine::clear_last_screen(id);
        engine::start_in_recovery(app, &record, &layout)?;
        let outcome = prepare_window(app, id, &title).and_then(|()| {
            let _lock = InputLock { app, id };
            let mut guest = LiveGuest::new(app, id);
            run(&mut guest, &account.user, &account.password, cancelled)
        });
        match outcome {
            Ok(()) => return finish(app, id, cancelled),
            Err(error) => {
                if !matches!(error, Failure::Cancelled) {
                    record_failure(app, id, &account.password, &error);
                }
                stop(app, id);
                match error {
                    Failure::Cancelled => return Err(CANCELLED.into()),
                    Failure::WrongLanguage(message) if attempt < ATTEMPTS => {
                        set_language()?;
                        failure = message;
                    }
                    Failure::Retry(message) if attempt < ATTEMPTS => failure = message,
                    Failure::Retry(message)
                    | Failure::WrongLanguage(message)
                    | Failure::Fatal(message) => return Err(message),
                }
            }
        }
    }
    Err(failure)
}

/// Keeps the last screen of a failed attempt for diagnosis and says where in
/// the log. The folder is Silo's log folder for the computer; the UI gets the
/// error message only.
fn record_failure(app: &AppHandle, id: &str, password: &str, error: &Failure) {
    let (Failure::Retry(message) | Failure::WrongLanguage(message) | Failure::Fatal(message)) =
        error
    else {
        return;
    };
    // Only a retryable failure happened before any credential was typed. After
    // that point recognition cannot be trusted to hide a secret, so the image
    // and the recognized text stay out of the log.
    let before_credentials = matches!(error, Failure::Retry(_) | Failure::WrongLanguage(_));
    let saved = app.path().app_log_dir().ok().and_then(|logs| {
        save_evidence(
            &logs.join("macos-computers").join(id),
            id,
            password,
            before_credentials,
        )
    });
    match saved {
        Some(path) => eprintln!(
            "macOS computer setup: turning off System Integrity Protection failed ({message}) Last screen: {}",
            path.display()
        ),
        None => eprintln!(
            "macOS computer setup: turning off System Integrity Protection failed ({message}) No screen was kept."
        ),
    }
}

/// How many failed attempts' files stay in a computer's evidence folder.
const EVIDENCE_KEPT: usize = 6;

/// Saves the last screen into `dir`: its image and recognized text before any
/// credential was typed, otherwise only the names of the known screens.
/// Returns the file to look at.
fn save_evidence(dir: &Path, id: &str, password: &str, full: bool) -> Option<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let (lines, _) = engine::save_last_screen(id, None)?;
    let (text, image) = if full {
        let (rows, shown) = redacted_rows(&lines, password);
        (rows, !shown)
    } else {
        let screen = Screen::new(lines);
        (screen.known_phrases().join("\n"), false)
    };
    let kept = write_evidence(dir, stamp, &text, |path| {
        image && engine::save_last_screen(id, Some(path)).is_some_and(|(_, saved)| saved)
    })?;
    prune_evidence(dir, EVIDENCE_KEPT);
    Some(kept)
}

/// Creates `path` readable by the owner only, failing if it exists.
fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// Writes one failed attempt's files into `dir` as a pair that appears whole or
/// not at all: both are written under temporary names, then renamed. `image`
/// writes the image to the path it is given and says whether it did. Returns
/// the image's path, or the text's when there is no image.
fn write_evidence(
    dir: &Path,
    stamp: u64,
    text: &str,
    image: impl FnOnce(&Path) -> bool,
) -> Option<PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .ok()?;
    let temp_text = dir.join(format!(".sip-failure-{stamp:012}.txt"));
    let temp_image = dir.join(format!(".sip-failure-{stamp:012}.png"));
    let text_path = dir.join(format!("sip-failure-{stamp:012}.txt"));
    let image_path = dir.join(format!("sip-failure-{stamp:012}.png"));
    let discard = |paths: &[&Path]| {
        for path in paths {
            let _ = std::fs::remove_file(path);
        }
    };
    if create_private(&temp_text)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .is_err()
    {
        discard(&[&temp_text]);
        return None;
    }
    // The image writer may replace the file, so the mode is set again afterwards.
    let has_image = create_private(&temp_image).is_ok()
        && image(&temp_image)
        && std::fs::set_permissions(&temp_image, std::fs::Permissions::from_mode(0o600)).is_ok();
    if !has_image {
        discard(&[&temp_image]);
        if std::fs::rename(&temp_text, &text_path).is_err() {
            discard(&[&temp_text]);
            return None;
        }
        return Some(text_path);
    }
    if std::fs::rename(&temp_image, &image_path).is_err() {
        discard(&[&temp_image, &temp_text]);
        return None;
    }
    if std::fs::rename(&temp_text, &text_path).is_err() {
        discard(&[&temp_text, &image_path]);
        return None;
    }
    Some(image_path)
}

/// The recognized rows of a screen, top to bottom and in lower case, with every
/// row that contains the password replaced; and whether any did.
fn redacted_rows(lines: &[TextLine], password: &str) -> (String, bool) {
    let secret = password.to_lowercase();
    let mut shown = false;
    let rows: Vec<String> = Screen::new(lines.to_vec())
        .rows
        .into_iter()
        .map(|row| {
            if !secret.is_empty() && row.text.contains(&secret) {
                shown = true;
                "[redacted]".to_string()
            } else {
                row.text
            }
        })
        .collect();
    (rows.join("\n"), shown)
}

/// Deletes the oldest evidence files beyond `kept` attempts (two files each).
fn prune_evidence(dir: &Path, kept: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("sip-failure-"))
        })
        .collect();
    let attempts: std::collections::BTreeSet<_> =
        files.iter().filter_map(|path| path.file_stem()).collect();
    let excess = attempts.len().saturating_sub(kept);
    let old: std::collections::BTreeSet<_> = attempts.into_iter().take(excess).collect();
    for path in &files {
        if path.file_stem().is_some_and(|stem| old.contains(stem)) {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn prepare_window(app: &AppHandle, id: &str, title: &str) -> Result<(), Failure> {
    super::show_display(app, id, title)
        .and_then(|()| engine::focus_display(app, id))
        .and_then(|_| engine::lock_input(app, id, LOCKED_SUBTITLE))
        .map_err(Failure::Fatal)
}

/// Gives the window its input back when the sequence ends, however it ends.
struct InputLock<'a> {
    app: &'a AppHandle,
    id: &'a str,
}

impl Drop for InputLock<'_> {
    fn drop(&mut self) {
        engine::unlock_input(self.app, self.id);
    }
}

/// Waits for the halted guest to stop and be released, forcing it if it does
/// not stop in time or the setup is cancelled.
fn finish(app: &AppHandle, id: &str, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    let deadline = Instant::now() + HALT_WAIT;
    while Instant::now() < deadline {
        if cancelled() {
            stop(app, id);
            return Err(CANCELLED.into());
        }
        if engine::wait_until_stopped(app, id, Duration::from_secs(1)) {
            return Ok(());
        }
    }
    stop(app, id);
    Err("The computer did not shut down after System Integrity Protection was changed.".into())
}

/// Force-stops the computer and waits for its machine to be released.
fn stop(app: &AppHandle, id: &str) {
    let _ = engine::force_stop(app, id);
    let _ = engine::wait_until_stopped(app, id, STOP_WAIT);
}

// MARK: Screens

/// Lines of recognized text that share a row of the screen, left to right.
struct Row {
    text: String,
}

/// The recognized text of one capture of the guest's screen.
struct Screen {
    lines: Vec<TextLine>,
    /// Lower-case rows, top to bottom.
    rows: Vec<Row>,
}

impl Screen {
    fn new(mut lines: Vec<TextLine>) -> Self {
        lines.sort_by(|a, b| (b.y + b.height).total_cmp(&(a.y + a.height)));
        let mut grouped: Vec<Vec<&TextLine>> = Vec::new();
        for line in &lines {
            let center = line.y + line.height / 2.0;
            match grouped.last_mut() {
                Some(row)
                    if row.iter().any(|other| {
                        (other.y + other.height / 2.0 - center).abs()
                            < 0.5 * other.height.max(line.height)
                    }) =>
                {
                    row.push(line)
                }
                _ => grouped.push(vec![line]),
            }
        }
        let rows = grouped
            .into_iter()
            .map(|mut row| {
                row.sort_by(|a, b| a.x.total_cmp(&b.x));
                Row {
                    text: row
                        .iter()
                        .map(|line| line.text.to_lowercase())
                        .collect::<Vec<_>>()
                        .join(" "),
                }
            })
            .collect();
        Self { lines, rows }
    }

    fn contains(&self, needle: &str) -> bool {
        self.rows.iter().any(|row| row.text.contains(needle))
    }

    /// The first line whose text is exactly `text`, ignoring case.
    fn line_equal(&self, text: &str) -> Option<&TextLine> {
        self.lines
            .iter()
            .find(|line| line.text.trim().to_lowercase() == text)
    }

    /// The language list Recovery shows first on a computer with no language
    /// set: language names in their own languages. The list does not depend on
    /// the language it is shown in.
    fn is_language_list(&self) -> bool {
        self.line_equal("english").is_some()
            && ["español", "français", "deutsch"]
                .into_iter()
                .any(|name| self.line_equal(name).is_some())
    }

    /// Whether any English label of Recovery's main window or its menu bar is on
    /// screen: the Utilities menu, or one of the window's options. A window
    /// whose menu word was misread still shows the others.
    fn is_english_main_window(&self) -> bool {
        const LABELS: [&str; 5] = [
            "utilities",
            "restore from time machine",
            "reinstall macos",
            "get help online",
            "disk utility",
        ];
        LABELS.iter().any(|label| self.contains(label))
            || (self.line_equal("edit").is_some() && self.line_equal("window").is_some())
    }

    /// How many list items sit above `item`: lines centered on it and as tall
    /// as it, which leaves out the larger heading. The first item is the one
    /// selected when the list appears.
    fn list_items_above(&self, item: &TextLine) -> usize {
        let center = item.x + item.width / 2.0;
        self.lines
            .iter()
            .filter(|line| {
                line.y > item.y
                    && (line.x + line.width / 2.0 - center).abs() < 0.1
                    && (line.height - item.height).abs() < 0.2 * item.height
            })
            .count()
    }

    /// The first line that starts with `prefix`.
    fn line_starting(&self, prefix: &str) -> Option<&TextLine> {
        self.lines
            .iter()
            .find(|line| line.text.to_lowercase().starts_with(prefix))
    }

    /// The rows inside the Terminal window: those below its title bar, or every
    /// row when no title bar is recognized.
    fn terminal_rows(&self) -> &[Row] {
        let title = self
            .rows
            .iter()
            .rposition(|row| row.text.contains("terminal") && row.text.contains('×'));
        title.map_or(&self.rows[..], |title| &self.rows[title + 1..])
    }

    /// The row the cursor is on: the active prompt, if there is one.
    fn active_row(&self) -> Option<&str> {
        self.terminal_rows().last().map(|row| row.text.as_str())
    }

    /// Whether Terminal waits at a shell prompt.
    fn at_shell_prompt(&self) -> bool {
        self.active_row().is_some_and(is_shell_prompt)
    }

    /// Names of the screens and prompts seen, for errors. Recognized text is
    /// never quoted: it can hold a computer name, and once credentials are
    /// typed it could hold more.
    fn known_phrases(&self) -> Vec<&'static str> {
        const KNOWN: [&str; 7] = [
            "options",
            "utilities",
            "english",
            "bash-",
            "y/n]",
            "authorized user",
            "password for user",
        ];
        KNOWN
            .into_iter()
            .filter(|phrase| self.contains(phrase))
            .collect()
    }
}

fn is_shell_prompt(row: &str) -> bool {
    let row = row.trim_end();
    row.contains("bash-") || row.ends_with('#') || row.ends_with('$')
}

/// What `csrutil` is asking or saying.
#[derive(Debug, PartialEq, Eq)]
enum Prompt {
    Confirm,
    Username,
    /// A password prompt, with the user it names when it names one.
    Password(Option<String>),
    Done,
    Rejected(&'static str),
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

/// Reads the Terminal: results anywhere in it, but a question only when it is
/// on the last row. A shell prompt on the last row means nothing is pending,
/// whatever scrolled by above it.
fn classify(screen: &Screen) -> Option<Prompt> {
    let rows = screen.terminal_rows();
    let anywhere = |phrase: &str| rows.iter().any(|row| row.text.contains(phrase));
    if DONE.iter().any(|phrase| anywhere(phrase)) {
        return Some(Prompt::Done);
    }
    if let Some(reason) = REJECTED.iter().find(|phrase| anywhere(phrase)) {
        return Some(Prompt::Rejected(reason));
    }
    let last = screen.active_row()?;
    if is_shell_prompt(last) {
        return None;
    }
    if last.contains("authorized user") {
        Some(Prompt::Username)
    } else if last.contains("password") {
        Some(Prompt::Password(password_user(last)))
    } else if last.contains("y/n]") {
        Some(Prompt::Confirm)
    } else {
        None
    }
}

/// The user in "enter password for user <name>:".
fn password_user(row: &str) -> Option<String> {
    const LEAD: &str = "password for user";
    let rest = row[row.rfind(LEAD)? + LEAD.len()..].trim_start();
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
    /// Recovery's main window is in a language other than English; setting the
    /// language and starting over fixes it.
    WrongLanguage(String),
    Fatal(String),
    Cancelled,
}

impl Failure {
    fn fatal(self) -> Self {
        match self {
            Self::Retry(message) | Self::WrongLanguage(message) | Self::Fatal(message) => {
                Self::Fatal(message)
            }
            Self::Cancelled => Self::Cancelled,
        }
    }
}

enum Check<T> {
    Found(T),
    Press(u16),
    /// Press Down this many times, then Return.
    Choose(usize),
    Rejected(String),
    Waiting,
}

struct Driver<'a, G: Guest> {
    guest: &'a mut G,
    cancelled: &'a dyn Fn() -> bool,
    keyboard: Keyboard,
    unreadable: usize,
    seen: Vec<&'static str>,
}

impl<G: Guest> Driver<'_, G> {
    fn check(&self) -> Result<(), Failure> {
        if (self.cancelled)() {
            Err(Failure::Cancelled)
        } else {
            Ok(())
        }
    }

    fn pause(&mut self, duration: Duration) -> Result<(), Failure> {
        let mut left = duration;
        while !left.is_zero() {
            self.check()?;
            let slice = left.min(CANCEL_SLICE);
            self.guest.pause(slice);
            left -= slice;
        }
        self.check()
    }

    fn send(&mut self, events: Vec<KeyEvent>) -> Result<(), Failure> {
        self.check()?;
        self.guest.keys(events).map_err(|message| {
            Failure::Fatal(format!("Silo lost the computer's setup window. {message}"))
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
            self.pause(KEY_GAP)?;
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
            self.check()?;
            self.guest.pointer(kind, x, y).map_err(|message| {
                Failure::Fatal(format!("Silo lost the computer's setup window. {message}"))
            })?;
            self.pause(Duration::from_millis(gap))?;
        }
        Ok(())
    }

    /// One reading of the screen. A screen that stays unreadable ends the setup.
    fn look(&mut self) -> Result<Option<Screen>, Failure> {
        match self.guest.screen() {
            Ok(lines) => {
                self.unreadable = 0;
                let screen = Screen::new(lines);
                self.seen = screen.known_phrases();
                Ok(Some(screen))
            }
            Err(_) => {
                self.unreadable += 1;
                if self.unreadable >= UNREADABLE_AFTER {
                    return Err(Failure::Fatal(
                        "Silo cannot read the computer's screen, so it stopped setting it up."
                            .into(),
                    ));
                }
                Ok(None)
            }
        }
    }

    /// What the last screen showed, as names of known screens only.
    fn describe(&self) -> String {
        if self.seen.is_empty() {
            "Nothing recognizable was on screen.".into()
        } else {
            format!("The screen showed: {}.", self.seen.join(", "))
        }
    }

    /// Polls the screen until `check` finds what it waits for.
    fn wait_for<T>(
        &mut self,
        what: &str,
        timeout: Duration,
        mut check: impl FnMut(&Screen) -> Check<T>,
    ) -> Result<T, Failure> {
        let deadline = self.guest.elapsed() + timeout;
        loop {
            self.check()?;
            if let Some(screen) = self.look()? {
                match check(&screen) {
                    Check::Found(found) => return Ok(found),
                    Check::Press(code) => {
                        self.press(code)?;
                        self.pause(Duration::from_secs(4))?;
                    }
                    Check::Choose(downs) => {
                        for _ in 0..downs {
                            self.press(input::DOWN)?;
                            self.pause(Duration::from_millis(300))?;
                        }
                        self.press(input::RETURN)?;
                        self.pause(Duration::from_secs(4))?;
                    }
                    Check::Rejected(reason) => {
                        return Err(Failure::Fatal(format!(
                            "Recovery refused the account ({reason}). {}",
                            self.describe()
                        )))
                    }
                    Check::Waiting => {}
                }
            }
            if self.guest.elapsed() >= deadline {
                return Err(Failure::Retry(format!(
                    "Recovery did not show {what}. {}",
                    self.describe()
                )));
            }
            self.pause(POLL)?;
        }
    }
}

/// Plays the whole sequence on a guest that has just been started in Recovery,
/// and returns once the halt command is sent.
fn run<G: Guest>(
    guest: &mut G,
    user: &str,
    password: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), Failure> {
    let mut driver = Driver {
        guest,
        cancelled,
        keyboard: Keyboard::default(),
        unreadable: 0,
        seen: Vec::new(),
    };
    pick_options(&mut driver)?;
    open_terminal(&mut driver)?;
    driver.type_line("csrutil disable")?;
    answer_prompts(&mut driver, user, password).map_err(Failure::fatal)?;
    driver.pause(Duration::from_secs(2))?;
    driver.type_line("halt").map_err(Failure::fatal)
}

/// Gets from the startup picker to Recovery's main window.
fn pick_options<G: Guest>(driver: &mut Driver<'_, G>) -> Result<(), Failure> {
    driver.wait_for("its startup options", PICKER_WAIT, |screen| {
        if screen.contains("options") {
            Check::Found(())
        } else {
            Check::Waiting
        }
    })?;
    // Skip "Macintosh HD" to reach "Options".
    for _ in 0..2 {
        driver.press(input::RIGHT)?;
        driver.pause(Duration::from_secs(1))?;
    }
    driver.press(input::RETURN)?;
    // The choice is made once. The list stays on screen for a few seconds after
    // Return on a slow start, and choosing again would count from English and
    // pick another language, which turns Recovery into one Silo cannot read.
    let mut chosen = false;
    let mut crowded = 0;
    let window = driver.wait_for("its main window", RECOVERY_WAIT, |screen| {
        if screen.contains("options") {
            // The startup picker is still up, whatever else it shows.
            crowded = 0;
            Check::Waiting
        } else if screen.is_english_main_window() {
            Check::Found(MainWindow::English)
        } else if screen.is_language_list() {
            crowded = 0;
            if chosen {
                return Check::Waiting;
            }
            chosen = true;
            screen
                .line_equal("english")
                .map_or(Check::Press(input::RETURN), |english| {
                    Check::Choose(screen.list_items_above(english))
                })
        } else if screen.rows.len() >= LOCALIZED_ROWS {
            // Recovery's window and menu bar, in a language Silo has no words for.
            crowded += 1;
            if crowded >= LOCALIZED_POLLS {
                Check::Found(MainWindow::Localized)
            } else {
                Check::Waiting
            }
        } else {
            crowded = 0;
            Check::Waiting
        }
    })?;
    match window {
        MainWindow::English => Ok(()),
        MainWindow::Localized => Err(Failure::WrongLanguage(
            "Recovery came up in a language other than English.".into(),
        )),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum MainWindow {
    English,
    Localized,
}

/// Rows of text that make a screen Recovery's main window when no known word
/// is on it, and how many reads in a row must agree.
const LOCALIZED_ROWS: usize = 6;
const LOCALIZED_POLLS: usize = 3;

/// Opens Terminal with its shortcut, or from the Utilities menu when the
/// shortcut does nothing, and waits for its shell prompt.
fn open_terminal<G: Guest>(driver: &mut Driver<'_, G>) -> Result<(), Failure> {
    let shortcut = driver.keyboard.chord(
        &[Modifier::Shift, Modifier::Command],
        input::KEY_T,
        Some('t'),
    );
    driver.send(shortcut)?;
    let shown = |screen: &Screen| {
        if screen.at_shell_prompt() {
            Check::Found(())
        } else {
            Check::Waiting
        }
    };
    match driver.wait_for("Terminal", SHORTCUT_WAIT, shown) {
        Ok(()) => return Ok(()),
        Err(Failure::Retry(_)) => {}
        Err(other) => return Err(other),
    }
    let menu = driver
        .look()?
        .and_then(|screen| screen.line_starting("utilities").cloned())
        .ok_or_else(|| {
            Failure::Retry(format!(
                "Recovery has no Utilities menu. {}",
                driver.describe()
            ))
        })?;
    driver.click(&menu)?;
    driver.pause(Duration::from_secs(1))?;
    let item = driver
        .look()?
        .and_then(|screen| screen.line_starting("terminal").cloned())
        .ok_or_else(|| {
            Failure::Retry(format!(
                "The Utilities menu has no Terminal. {}",
                driver.describe()
            ))
        })?;
    driver.click(&item)?;
    driver.wait_for("Terminal", TERMINAL_WAIT, shown)
}

/// Answers `csrutil`'s questions until it reports the result. macOS 26 may ask
/// for a user name before the password. The password is typed only at a
/// password prompt that is the last row of the Terminal and names the
/// computer's account (compared without regard to case, since recognition
/// cannot be trusted with it), or follows the user name Silo typed.
fn answer_prompts<G: Guest>(
    driver: &mut Driver<'_, G>,
    user: &str,
    password: &str,
) -> Result<(), Failure> {
    let (mut confirmed, mut named, mut authenticated) = (false, false, false);
    loop {
        let wait = if authenticated {
            RESULT_WAIT
        } else {
            PROMPT_WAIT
        };
        let prompt =
            driver.wait_for("a csrutil prompt", wait, |screen| match classify(screen) {
                Some(Prompt::Done) => Check::Found(Prompt::Done),
                Some(Prompt::Rejected(reason)) => Check::Rejected(reason.into()),
                Some(Prompt::Confirm) if !confirmed => Check::Found(Prompt::Confirm),
                Some(Prompt::Username) if !named => Check::Found(Prompt::Username),
                Some(Prompt::Password(who)) if !authenticated => {
                    Check::Found(Prompt::Password(who))
                }
                _ => Check::Waiting,
            })?;
        match prompt {
            Prompt::Done => return Ok(()),
            Prompt::Confirm => {
                confirmed = true;
                driver.type_line("y")?;
            }
            Prompt::Username => {
                named = true;
                driver.type_line(user)?;
            }
            Prompt::Password(who) => {
                let expected = user.to_lowercase();
                let right_account = match &who {
                    Some(who) => *who == expected,
                    None => named,
                };
                if !right_account {
                    return Err(Failure::Fatal(
                        "Recovery asked for the password of an account other than the computer's, so it does not know the account."
                            .into(),
                    ));
                }
                authenticated = true;
                driver.type_line(password)?;
            }
            Prompt::Rejected(_) => unreachable!("a rejection ends the wait with an error"),
        }
        driver.pause(Duration::from_secs(2))?;
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

    fn screen(rows: &[&str]) -> Screen {
        Screen::new(
            rows.iter()
                .enumerate()
                .map(|(index, text)| line(text, 0.1, 0.9 - index as f64 * 0.05))
                .collect(),
        )
    }

    const CONFIRM: &str = "Allow booting unsigned operating systems and any kernel extensions for OS \"Macintosh HD\"? [y/n]:";

    #[test]
    fn the_last_row_decides_the_prompt() {
        assert_eq!(
            classify(&screen(&["-bash-3.2# csrutil disable", CONFIRM])),
            Some(Prompt::Confirm)
        );
        assert_eq!(
            classify(&screen(&[
                "[y/n]: y",
                "Enter a username of an authorized user:"
            ])),
            Some(Prompt::Username)
        );
        assert_eq!(
            classify(&screen(&["authorized user: silo", "Password:"])),
            Some(Prompt::Password(None))
        );
        assert_eq!(
            classify(&screen(&["[y/n]: y", "Enter password for user silo:"])),
            Some(Prompt::Password(Some("silo".into())))
        );
    }

    #[test]
    fn a_stale_prompt_above_a_shell_prompt_is_not_pending() {
        let stale = screen(&[
            CONFIRM,
            "[y/n]: y",
            "Enter password for user silo:",
            "^C",
            "-bash-3.2#",
        ]);
        assert_eq!(classify(&stale), None);
        assert!(stale.at_shell_prompt());
    }

    #[test]
    fn a_command_being_typed_is_not_a_prompt() {
        assert_eq!(
            classify(&screen(&[
                "Enter password for user silo:",
                "-bash-3.2# csrutil disable"
            ])),
            None
        );
    }

    #[test]
    fn a_prompt_split_across_recognized_lines_is_one_row() {
        let split = Screen::new(vec![
            line("Enter password for user", 0.1, 0.5),
            line("_mbsetupuser:", 0.4, 0.5),
        ]);
        assert_eq!(
            classify(&split),
            Some(Prompt::Password(Some("_mbsetupuser".into())))
        );
    }

    #[test]
    fn results_override_prompts() {
        assert_eq!(
            classify(&screen(&[
                "Password:",
                "System Integrity Protection is off."
            ])),
            Some(Prompt::Done)
        );
        assert!(matches!(
            classify(&screen(&[
                "password for user silo:",
                "csrutil: failed to set credential: authentication failure.",
                "-bash-3.2#"
            ])),
            Some(Prompt::Rejected(_))
        ));
        assert_eq!(classify(&screen(&["-bash-3.2#"])), None);
    }

    #[test]
    fn only_rows_below_the_terminal_title_count() {
        let terminal = screen(&[
            "Recovery  File  Utilities",
            "Enter password for user silo:",
            "Terminal — -bash — 120×30",
            "-bash-3.2#",
        ]);
        assert_eq!(classify(&terminal), None);
        assert_eq!(terminal.terminal_rows().len(), 1);
    }

    #[test]
    fn a_line_is_found_by_its_start() {
        let screen = Screen::new(vec![
            line("Recovery", 0.1, 0.9),
            line("Utilities", 0.3, 0.9),
        ]);
        assert_eq!(screen.line_starting("utilities").unwrap().x, 0.3);
        assert!(screen.line_starting("terminal").is_none());
        assert_eq!(screen.rows.len(), 1);
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
        prompt_user: &'static str,
        cancel_confirm: bool,
        language_list: bool,
        /// Recovery's main window is in French.
        french: bool,
        /// How long the language list stays after the language is chosen.
        language_lag: Duration,
        chosen_at: Option<Duration>,
        downs: usize,
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
        Stale,
        Language,
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
                prompt_user: "silo",
                cancel_confirm: false,
                language_list: false,
                french: false,
                language_lag: Duration::ZERO,
                chosen_at: None,
                downs: 0,
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
                (Phase::Confirm, "y") if self.cancel_confirm => self.enter(Phase::Stale),
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
            if self.phase == Phase::Language {
                let heading = TextLine {
                    height: 0.05,
                    ..line("Langue", 0.45, 0.95)
                };
                let mut lines = vec![heading];
                for (index, text) in ["Français", "English (UK)", "English", "Español"]
                    .iter()
                    .enumerate()
                {
                    lines.push(line(text, 0.45, 0.8 - index as f64 * 0.05));
                }
                return Ok(lines);
            }
            let prompt = format!("Enter password for user {}:", self.prompt_user);
            let rows: Vec<&str> = match self.phase {
                Phase::Picker if self.now >= self.entered + Duration::from_secs(12) => {
                    vec!["Macintosh HD", "Options", "Shut Down"]
                }
                Phase::Main if self.french => vec![
                    "Récupération",
                    "Fichier",
                    "Édition",
                    "Utilitaires",
                    "Fenêtre",
                    "Restaurer à partir de Time Machine",
                    "Réinstaller macOS Tahoe",
                    "Navigateur Web",
                    "Utilitaire de disque",
                    "Continuer",
                ],
                Phase::Main => {
                    if self.menu_open {
                        vec![
                            "Recovery",
                            "Utilities",
                            "Startup Security Utility",
                            "Terminal ⇧⌘T",
                        ]
                    } else {
                        vec!["Recovery", "File", "Utilities", "Restore from Time Machine"]
                    }
                }
                Phase::Terminal => vec!["Terminal", "-bash-3.2#"],
                Phase::Confirm => vec!["-bash-3.2# csrutil disable", CONFIRM],
                Phase::Username => vec!["[y/n]: y", "Enter a username of an authorized user:"],
                Phase::Password if self.username_prompt => {
                    vec!["authorized user: silo", "Password:"]
                }
                Phase::Password => vec!["[y/n]: y", &prompt],
                Phase::Result => vec!["System Integrity Protection is off."],
                Phase::Rejected => vec![
                    "csrutil: failed to set credential: Authentication failure.",
                    "-bash-3.2#",
                ],
                Phase::Stale => vec!["[y/n]: y", &prompt, "^C", "-bash-3.2#"],
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
                            let phase = match (self.picked == 2, self.language_list) {
                                (false, _) => Phase::Halted,
                                (true, true) => Phase::Language,
                                (true, false) => Phase::Booting,
                            };
                            self.enter(phase);
                        }
                        (Phase::Language, input::DOWN) => self.downs += 1,
                        (Phase::Language, input::RETURN) => {
                            // Without English chosen, Recovery would come up localized.
                            if self.downs == 2 {
                                self.chosen_at = Some(self.now);
                            } else {
                                self.enter(Phase::Halted);
                            }
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
            if let Some(at) = self.chosen_at {
                if self.phase == Phase::Language && self.now >= at + self.language_lag {
                    self.enter(Phase::Booting);
                }
            }
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

    fn never() -> bool {
        false
    }

    #[test]
    fn the_modern_flow_asks_for_a_user_then_a_password() {
        let mut guest = Recovery::new();
        run(&mut guest, "silo", "secret", &never).unwrap();
        assert_eq!(guest.phase, Phase::Halted);
        assert_eq!(
            lines_typed(&guest),
            ["csrutil disable", "y", "silo", "secret", "halt"]
        );
        assert!(guest.keys.contains(&"shortcut".to_string()));
    }

    #[test]
    fn the_older_flow_asks_for_the_password_directly() {
        let mut guest = Recovery::new();
        guest.username_prompt = false;
        run(&mut guest, "silo", "secret", &never).unwrap();
        assert_eq!(guest.phase, Phase::Halted);
        assert_eq!(
            lines_typed(&guest),
            ["csrutil disable", "y", "secret", "halt"]
        );
    }

    #[test]
    fn terminal_is_opened_from_the_menu_when_the_shortcut_does_nothing() {
        let mut guest = Recovery::new();
        guest.shortcut_works = false;
        run(&mut guest, "silo", "secret", &never).unwrap();
        assert!(guest.keys.contains(&"menu".to_string()));
        assert_eq!(guest.phase, Phase::Halted);
    }

    #[test]
    fn the_language_list_is_answered_with_english_by_keyboard() {
        let mut guest = Recovery::new();
        guest.language_list = true;
        run(&mut guest, "silo", "secret", &never).unwrap();
        assert_eq!(guest.phase, Phase::Halted);
        assert!(lines_typed(&guest).contains(&"secret".to_string()));
    }

    #[test]
    fn a_language_list_that_lingers_after_the_choice_is_not_answered_again() {
        let mut guest = Recovery::new();
        guest.language_list = true;
        guest.language_lag = Duration::from_secs(9);
        run(&mut guest, "silo", "secret", &never).unwrap();
        assert_eq!(guest.phase, Phase::Halted);
        assert_eq!(guest.downs, 2);
        assert!(lines_typed(&guest).contains(&"secret".to_string()));
    }

    #[test]
    fn a_main_window_in_another_language_asks_for_the_language_to_be_set() {
        let mut guest = Recovery::new();
        guest.french = true;
        let error = run(&mut guest, "silo", "secret", &never).unwrap_err();
        assert!(matches!(error, Failure::WrongLanguage(_)), "{error:?}");
        assert!(guest.keys.is_empty() || !lines_typed(&guest).contains(&"secret".to_string()));
        assert!(!lines_typed(&guest).contains(&"csrutil disable".to_string()));
    }

    #[test]
    fn an_english_window_with_a_misread_menu_word_is_not_another_language() {
        let mut rows = vec!["Recovery", "File", "Edit", "Utiities", "Window"];
        rows.extend([
            "Restore from Time Machine",
            "Reinstall macOS Tahoe",
            "Disk Utility",
            "Continue",
        ]);
        rows.retain(|row| *row != "Continue");
        assert!(screen(&rows).is_english_main_window());
        assert!(screen(&["Reinstall macOS Tahoe"]).is_english_main_window());
        assert!(screen(&["Edit", "Window"]).is_english_main_window());
        assert!(!screen(&[
            "Récupération",
            "Fichier",
            "Continuer",
            "Utilitaire de disque"
        ])
        .is_english_main_window());
    }

    #[test]
    fn a_picker_that_lingers_after_return_is_not_the_main_window() {
        assert!(!screen(&["Macintosh HD", "Options", "Continue"]).is_english_main_window());
        struct Lingering(Recovery);
        impl Guest for Lingering {
            fn screen(&mut self) -> Result<Vec<TextLine>, String> {
                if self.0.phase == Phase::Booting {
                    return Ok(vec![
                        line("Macintosh HD", 0.2, 0.5),
                        line("Options", 0.4, 0.5),
                        line("Continue", 0.4, 0.4),
                        line("Disk Utility", 0.4, 0.3),
                    ]);
                }
                self.0.screen()
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
        let mut guest = Lingering(Recovery::new());
        run(&mut guest, "silo", "secret", &never).unwrap();
        assert!(!guest.0.keys.contains(&"shortcut".to_string()) || guest.0.phase == Phase::Halted);
        assert_eq!(guest.0.phase, Phase::Halted);
    }

    #[test]
    fn a_language_failure_is_final_once_credentials_could_have_been_typed() {
        assert!(matches!(
            Failure::WrongLanguage("x".into()).fatal(),
            Failure::Fatal(_)
        ));
    }

    #[test]
    fn evidence_hides_rows_with_the_password_and_the_image_that_shows_it() {
        let lines = vec![
            line("Recovery", 0.1, 0.9),
            line("my Secret", 0.1, 0.8),
            line("-bash-3.2#", 0.1, 0.7),
        ];
        let (text, shown) = redacted_rows(&lines, "secret");
        assert!(shown);
        assert_eq!(text, "recovery\n[redacted]\n-bash-3.2#");
        let (text, shown) = redacted_rows(&lines, "other");
        assert!(!shown);
        assert!(text.contains("my secret"));
        assert!(!redacted_rows(&lines, "").1);
    }

    #[test]
    fn evidence_is_private_and_written_as_a_whole_pair() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("logs");
        // Like the image writer, replace the file the writer was given.
        let kept = write_evidence(&folder, 7, "recovery", |path| {
            std::fs::remove_file(path).is_ok() && std::fs::write(path, b"png").is_ok()
        })
        .unwrap();
        assert_eq!(kept, folder.join("sip-failure-000000000007.png"));
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&folder), 0o700);
        assert_eq!(mode(&kept), 0o600);
        assert_eq!(mode(&kept.with_extension("txt")), 0o600);
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 2);
    }

    #[test]
    fn evidence_without_an_image_is_only_the_text_and_leaves_no_partials() {
        let dir = tempfile::tempdir().unwrap();
        let kept = write_evidence(dir.path(), 8, "recovery", |_| false).unwrap();
        assert_eq!(kept, dir.path().join("sip-failure-000000000008.txt"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_failed_write_cleans_up_its_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        // A directory in the text's place makes the final rename fail.
        std::fs::create_dir(dir.path().join("sip-failure-000000000009.txt")).unwrap();
        let kept = write_evidence(dir.path(), 9, "recovery", |path| {
            std::fs::write(path, b"png").is_ok()
        });
        assert!(kept.is_none());
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["sip-failure-000000000009.txt"]);
    }

    #[test]
    fn old_evidence_is_pruned_by_attempt() {
        let dir = tempfile::tempdir().unwrap();
        for stamp in 1..=4 {
            for extension in ["png", "txt"] {
                std::fs::write(
                    dir.path()
                        .join(format!("sip-failure-{stamp:012}.{extension}")),
                    "",
                )
                .unwrap();
            }
        }
        std::fs::write(dir.path().join("other.txt"), "").unwrap();
        prune_evidence(dir.path(), 2);
        let mut names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "other.txt",
                "sip-failure-000000000003.png",
                "sip-failure-000000000003.txt",
                "sip-failure-000000000004.png",
                "sip-failure-000000000004.txt"
            ]
        );
    }

    #[test]
    fn the_language_list_is_recognized_by_native_names() {
        assert!(screen(&["Langue", "Français", "English", "Español"]).is_language_list());
        let list = screen(&["Language", "Français", "English (UK)", "English", "Español"]);
        let english = list.line_equal("english").unwrap();
        // Heading, Français and English (UK) share the list's center; the heading
        // is excluded only when it is taller, which `screen` does not model.
        assert_eq!(list.list_items_above(english), 3);
        assert!(!screen(&["Macintosh HD", "Options"]).is_language_list());
        assert!(!screen(&["English (UK)", "English"]).is_language_list());
    }

    #[test]
    fn a_rejected_password_fails_without_retrying() {
        let mut guest = Recovery::new();
        guest.password_ok = false;
        let error = run(&mut guest, "silo", "secret", &never).unwrap_err();
        match error {
            Failure::Fatal(message) => {
                assert!(message.contains("authentication failure"), "{message}");
                assert!(!message.contains("secret"));
            }
            other => panic!("{other:?}"),
        }
        assert_ne!(guest.phase, Phase::Halted);
    }

    #[test]
    fn a_password_prompt_for_another_account_gets_no_password() {
        for name in ["_mbsetupuser", "bob"] {
            let mut guest = Recovery::new();
            guest.username_prompt = false;
            guest.prompt_user = name;
            let error = run(&mut guest, "silo", "secret", &never).unwrap_err();
            assert!(matches!(error, Failure::Fatal(_)), "{name}");
            assert!(
                !lines_typed(&guest).contains(&"secret".to_string()),
                "{name}"
            );
        }
    }

    #[test]
    fn the_account_name_is_matched_without_regard_to_case() {
        let mut guest = Recovery::new();
        guest.username_prompt = false;
        guest.prompt_user = "Silo";
        run(&mut guest, "silo", "secret", &never).unwrap();
    }

    #[test]
    fn a_stale_prompt_never_receives_the_password() {
        let mut guest = Recovery::new();
        guest.username_prompt = false;
        guest.cancel_confirm = true;
        let error = run(&mut guest, "silo", "secret", &never).unwrap_err();
        assert!(matches!(error, Failure::Fatal(_)));
        assert!(!lines_typed(&guest).contains(&"secret".to_string()));
    }

    #[test]
    fn recovery_that_never_appears_is_a_retryable_failure_that_quotes_no_text() {
        struct Dead(Recovery);
        impl Guest for Dead {
            fn screen(&mut self) -> Result<Vec<TextLine>, String> {
                Ok(vec![line("Setting up My Options Mac", 0.5, 0.5)])
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
        let error = run(&mut guest, "silo", "secret", &never).unwrap_err();
        match error {
            Failure::Retry(message) => {
                assert!(message.contains("options"), "{message}");
                assert!(!message.contains("my options mac"), "{message}");
            }
            other => panic!("{other:?}"),
        }
        assert!(guest.0.now >= PICKER_WAIT);
    }

    #[test]
    fn an_unreadable_screen_stops_the_setup_without_typing_anything() {
        let mut guest = Recovery::new();
        guest.readable = false;
        let error = run(&mut guest, "silo", "secret", &never).unwrap_err();
        assert!(matches!(error, Failure::Fatal(message) if message.contains("cannot read")));
        assert!(guest.keys.is_empty());
    }

    #[test]
    fn a_screen_that_goes_dark_midway_types_no_credentials() {
        struct Fails(Recovery, bool);
        impl Guest for Fails {
            fn screen(&mut self) -> Result<Vec<TextLine>, String> {
                if self.0.phase == Phase::Confirm {
                    self.1 = true;
                }
                if self.1 {
                    Err("gone".into())
                } else {
                    self.0.screen()
                }
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
        let mut guest = Fails(Recovery::new(), false);
        assert!(run(&mut guest, "silo", "secret", &never).is_err());
        assert_eq!(lines_typed(&guest.0), ["csrutil disable"]);
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
            run(&mut Closed, "silo", "secret", &never),
            Err(Failure::Fatal(_))
        ));
    }

    #[test]
    fn cancelling_stops_at_once_and_types_no_credentials() {
        for cancel_at in [0usize, 5, 40, 120] {
            let mut guest = Recovery::new();
            let polls = std::cell::Cell::new(0usize);
            let cancelled = || {
                polls.set(polls.get() + 1);
                polls.get() > cancel_at
            };
            let result = run(&mut guest, "silo", "secret", &cancelled);
            assert!(matches!(result, Err(Failure::Cancelled)), "{cancel_at}");
            assert!(!lines_typed(&guest).contains(&"secret".to_string()));
            assert_ne!(guest.phase, Phase::Halted, "{cancel_at}");
        }
    }
}
