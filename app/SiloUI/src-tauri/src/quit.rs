//! Quit for a client of the engine (95 A-2).
//!
//! Quitting Silo never stops computers on its own. When computers are running the user
//! chooses Cancel, Keep running, or Stop and quit; the engine does the stopping
//! (`engine.shutdown`), the app saves its settings and exits. An update or a crash of the
//! app leaves the engine and its computers running.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;

/// How long to wait for the webview to take over the settings flush before saving natively.
pub(crate) const FRONTEND_FLUSH_FALLBACK: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Cancel,
    KeepRunning,
    StopAndQuit,
}

impl Choice {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "cancel" => Some(Self::Cancel),
            "keep" => Some(Self::KeepRunning),
            "stop" => Some(Self::StopAndQuit),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuitRequest {
    pub request_id: u64,
    /// Running computers; empty when their state could not be read.
    pub computers: Vec<String>,
}

/// Everything Quit needs from the app and the engine. The production host talks to the
/// webview and the engine; tests script it.
pub(crate) trait QuitHost: Send + Sync + 'static {
    /// Names of the running computers on this device.
    fn running_computers(&self) -> Result<Vec<String>, String>;
    /// Shows the Quit prompt in the main window; false when it cannot be shown.
    fn ask(&self, request: &QuitRequest) -> bool;
    /// Asks the webview to save its pending settings changes.
    fn request_flush(&self);
    fn announce(&self, shutting_down: bool);
    /// `engine.shutdown {stopComputers: true}`, waited for.
    fn stop_computers(&self) -> Result<(), String>;
    /// `settings.flush` on the engine.
    fn flush_settings(&self) -> Result<(), String>;
    /// The graceful path finished: let AppKit or the process end.
    fn exit(&self);
    /// Quit was cancelled or failed: tell AppKit Silo keeps running.
    fn cancelled(&self);
    /// Shows `message` to the user in the main window.
    fn explain(&self, message: String);
}

#[derive(Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Asking(u64),
    Flushing {
        generation: u64,
        acknowledged: bool,
        stop: bool,
    },
    Exiting,
}

struct Inner {
    phase: Phase,
    enabled: bool,
    next_request: u64,
    generation: u64,
    session_ending: bool,
}

pub(crate) struct Quit {
    host: Arc<dyn QuitHost>,
    inner: Mutex<Inner>,
    fallback: Duration,
}

impl Quit {
    pub(crate) fn new(host: Arc<dyn QuitHost>) -> Arc<Self> {
        Self::with_fallback(host, FRONTEND_FLUSH_FALLBACK)
    }

    pub(crate) fn with_fallback(host: Arc<dyn QuitHost>, fallback: Duration) -> Arc<Self> {
        Arc::new(Self {
            host,
            inner: Mutex::new(Inner {
                phase: Phase::Idle,
                enabled: false,
                next_request: 0,
                generation: 0,
                session_ending: false,
            }),
            fallback,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The main window can answer the Quit prompt.
    pub(crate) fn enable_confirmation(&self) {
        self.lock().enabled = true;
    }

    /// Whether Quit is under way (the webview shows its quitting overlay).
    pub(crate) fn active(&self) -> bool {
        matches!(self.lock().phase, Phase::Flushing { .. } | Phase::Exiting)
    }

    /// The graceful path already approved the exit; the process may end now.
    pub(crate) fn exit_approved(&self) -> bool {
        matches!(self.lock().phase, Phase::Exiting)
    }

    /// AppKit should wait for Silo's own path unless it already approved the exit.
    pub(crate) fn accepts_terminate_request(&self) -> bool {
        !matches!(self.lock().phase, Phase::Exiting)
    }

    /// Every user Quit entry point lands here.
    pub(crate) fn request(self: &Arc<Self>) {
        let this = self.clone();
        std::thread::spawn(move || this.request_blocking());
    }

    pub(crate) fn request_blocking(self: &Arc<Self>) {
        {
            let inner = self.lock();
            if !matches!(inner.phase, Phase::Idle) {
                // A repeated Quit while the prompt is open or Quit is under way changes nothing.
                return;
            }
        }
        let enabled = self.lock().enabled;
        let running = self.host.running_computers();
        let names = match &running {
            Ok(names) if names.is_empty() => {
                self.begin_exit(false);
                return;
            }
            Ok(names) => names.clone(),
            // Unknown: the prompt warns that quitting may leave computers running.
            Err(_) => Vec::new(),
        };
        if !enabled {
            self.begin_exit(false);
            return;
        }
        let request_id = {
            let mut inner = self.lock();
            if !matches!(inner.phase, Phase::Idle) {
                return;
            }
            inner.next_request += 1;
            let id = inner.next_request;
            inner.phase = Phase::Asking(id);
            id
        };
        let request = QuitRequest {
            request_id,
            computers: names,
        };
        if !self.host.ask(&request) {
            // Without a prompt there is nothing to wait for; computers are left as they are.
            self.begin_exit(false);
        }
    }

    /// The user already chose (the tray's own confirmation): skip the prompt.
    pub(crate) fn quit_chosen(self: &Arc<Self>, choice: Choice) {
        match choice {
            Choice::Cancel => {}
            Choice::KeepRunning => self.begin_exit(false),
            Choice::StopAndQuit => self.begin_exit(true),
        }
    }

    /// The webview's answer to the prompt.
    pub(crate) fn answer(self: &Arc<Self>, request_id: u64, choice: Choice) -> Result<(), String> {
        {
            let mut inner = self.lock();
            match inner.phase {
                Phase::Asking(id) if id == request_id => {
                    if choice == Choice::Cancel {
                        inner.phase = Phase::Idle;
                    }
                }
                // An answer to a prompt that no longer applies is ignored.
                _ => return Ok(()),
            }
        }
        match choice {
            Choice::Cancel => self.host.cancelled(),
            Choice::KeepRunning => self.begin_exit(false),
            Choice::StopAndQuit => self.begin_exit(true),
        }
        Ok(())
    }

    fn begin_exit(self: &Arc<Self>, stop: bool) {
        let generation = {
            let mut inner = self.lock();
            if matches!(inner.phase, Phase::Flushing { .. } | Phase::Exiting) {
                return;
            }
            inner.generation += 1;
            inner.phase = Phase::Flushing {
                generation: inner.generation,
                acknowledged: false,
                stop,
            };
            inner.generation
        };
        self.host.announce(true);
        self.host.request_flush();
        let this = self.clone();
        let fallback = self.fallback;
        std::thread::spawn(move || {
            // Only a webview that never acknowledges may use the fallback. A responsive one
            // can take as long as it needs to drain its pending changes.
            std::thread::sleep(fallback);
            this.finish(false, Some(generation));
        });
    }

    /// The webview took over the flush.
    pub(crate) fn begin_flush(&self) -> Result<(), String> {
        let mut inner = self.lock();
        match &mut inner.phase {
            Phase::Flushing { acknowledged, .. } if !*acknowledged => {
                *acknowledged = true;
                Ok(())
            }
            _ => Err("A settings flush is not awaiting acknowledgment".into()),
        }
    }

    /// The webview saved everything.
    pub(crate) fn complete_flush(self: &Arc<Self>) {
        self.finish(true, None);
    }

    /// The webview could not save: stay open.
    pub(crate) fn cancel_flush(self: &Arc<Self>) {
        let session_ending = self.lock().session_ending;
        if session_ending {
            self.finish(true, None);
            return;
        }
        if self.cancel_phase() {
            self.host.explain("Silo stayed open because pending changes could not be saved. Check the reported save error, then choose Quit Silo again. Computers keep running.".into());
        }
    }

    fn cancel_phase(&self) -> bool {
        let cancelled = {
            let mut inner = self.lock();
            if matches!(inner.phase, Phase::Flushing { .. }) {
                inner.phase = Phase::Idle;
                true
            } else {
                false
            }
        };
        if cancelled {
            self.host.announce(false);
            self.host.cancelled();
        }
        cancelled
    }

    fn finish(self: &Arc<Self>, frontend_completed: bool, generation: Option<u64>) {
        let (stop, session_ending) = {
            let mut inner = self.lock();
            let Phase::Flushing {
                generation: current,
                acknowledged,
                stop,
            } = inner.phase
            else {
                return;
            };
            let claimable = frontend_completed
                || (!acknowledged && generation.is_none_or(|expected| expected == current));
            if !claimable {
                return;
            }
            inner.phase = Phase::Exiting;
            (stop, inner.session_ending)
        };
        // Settings first: stopping computers can end the engine, which then cannot save them.
        if let Err(error) = self.host.flush_settings() {
            if !session_ending {
                self.reopen();
                self.host.explain(format!("Silo could not save its settings.\n\n{error}\n\nResolve the storage issue and choose Quit Silo again."));
                return;
            }
            eprintln!(
                "Silo is exiting because the session ended; settings were not saved: {error}"
            );
        }
        if stop {
            if let Err(error) = self.host.stop_computers() {
                if !session_ending {
                    self.reopen();
                    self.host.explain(format!("Silo stayed open because its computers could not be stopped.\n\n{error}\n\nCheck the affected computers and choose Quit Silo again."));
                    return;
                }
                eprintln!("Silo is exiting because the session ended: {error}");
            }
        }
        self.host.exit();
    }

    /// Leaves the exit path after a failure so Quit can be chosen again.
    fn reopen(&self) {
        self.lock().phase = Phase::Idle;
        self.host.announce(false);
        self.host.cancelled();
    }

    /// Logout, restart, shutdown or SIGTERM: no prompt, a bounded save, and the engine
    /// ends its own session. A failed save never keeps the session waiting.
    pub(crate) fn end_session(self: &Arc<Self>, budget: Duration) {
        {
            let mut inner = self.lock();
            if matches!(inner.phase, Phase::Exiting) {
                return;
            }
            inner.session_ending = true;
            if matches!(inner.phase, Phase::Asking(_)) {
                inner.phase = Phase::Idle;
            }
        }
        let deadline_host = self.host.clone();
        std::thread::spawn(move || {
            std::thread::sleep(budget);
            eprintln!("Silo is exiting because the session shutdown deadline elapsed.");
            deadline_host.exit();
        });
        self.begin_exit(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Script {
        running: Mutex<Result<Vec<String>, String>>,
        ask_fails: bool,
        stop_error: Mutex<Option<String>>,
        flush_error: Mutex<Option<String>>,
        log: Mutex<Vec<String>>,
        exits: AtomicU32,
    }

    impl Script {
        fn new(running: Result<Vec<&str>, &str>) -> Arc<Self> {
            Arc::new(Self {
                running: Mutex::new(
                    running
                        .map(|names| names.into_iter().map(String::from).collect())
                        .map_err(String::from),
                ),
                ask_fails: false,
                stop_error: Mutex::new(None),
                flush_error: Mutex::new(None),
                log: Mutex::new(Vec::new()),
                exits: AtomicU32::new(0),
            })
        }

        fn log(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }

    impl QuitHost for Script {
        fn running_computers(&self) -> Result<Vec<String>, String> {
            self.running.lock().unwrap().clone()
        }

        fn ask(&self, request: &QuitRequest) -> bool {
            self.log.lock().unwrap().push(format!(
                "ask {} {:?}",
                request.request_id, request.computers
            ));
            !self.ask_fails
        }

        fn request_flush(&self) {
            self.log.lock().unwrap().push("flush-request".into());
        }

        fn announce(&self, shutting_down: bool) {
            self.log
                .lock()
                .unwrap()
                .push(format!("announce {shutting_down}"));
        }

        fn stop_computers(&self) -> Result<(), String> {
            self.log.lock().unwrap().push("stop".into());
            self.stop_error.lock().unwrap().clone().map_or(Ok(()), Err)
        }

        fn flush_settings(&self) -> Result<(), String> {
            self.log.lock().unwrap().push("settings".into());
            self.flush_error.lock().unwrap().clone().map_or(Ok(()), Err)
        }

        fn exit(&self) {
            self.exits.fetch_add(1, Ordering::SeqCst);
            self.log.lock().unwrap().push("exit".into());
        }

        fn cancelled(&self) {
            self.log.lock().unwrap().push("cancelled".into());
        }

        fn explain(&self, message: String) {
            self.log.lock().unwrap().push(format!(
                "explain {}",
                message.lines().next().unwrap_or_default()
            ));
        }
    }

    fn quit(script: &Arc<Script>) -> Arc<Quit> {
        let quit = Quit::with_fallback(script.clone(), Duration::from_secs(30));
        quit.enable_confirmation();
        quit
    }

    #[test]
    fn with_no_running_computers_quit_saves_and_exits_without_stopping_anything() {
        let script = Script::new(Ok(vec![]));
        let quit = quit(&script);
        quit.request_blocking();
        assert!(quit.active());
        quit.begin_flush().unwrap();
        quit.complete_flush();
        assert_eq!(
            script.log(),
            ["announce true", "flush-request", "settings", "exit"]
        );
    }

    #[test]
    fn running_computers_prompt_and_keep_running_never_stops_them() {
        let script = Script::new(Ok(vec!["dev", "linux-1"]));
        let quit = quit(&script);
        quit.request_blocking();
        assert_eq!(script.log(), [r#"ask 1 ["dev", "linux-1"]"#]);
        // A repeated Quit while the prompt is open is ignored.
        quit.request_blocking();
        assert_eq!(script.log().len(), 1);
        quit.answer(1, Choice::KeepRunning).unwrap();
        quit.begin_flush().unwrap();
        quit.complete_flush();
        let log = script.log();
        assert!(!log.contains(&"stop".to_string()));
        assert_eq!(log.last().unwrap(), "exit");
    }

    #[test]
    fn stop_and_quit_stops_computers_after_the_webview_saved() {
        let script = Script::new(Ok(vec!["dev"]));
        let quit = quit(&script);
        quit.request_blocking();
        quit.answer(1, Choice::StopAndQuit).unwrap();
        quit.begin_flush().unwrap();
        quit.complete_flush();
        assert_eq!(&script.log()[3..], ["settings", "stop", "exit"]);
    }

    #[test]
    fn cancel_keeps_silo_open_and_a_stale_answer_is_ignored() {
        let script = Script::new(Ok(vec!["dev"]));
        let quit = quit(&script);
        quit.request_blocking();
        quit.answer(1, Choice::Cancel).unwrap();
        assert_eq!(script.log().last().unwrap(), "cancelled");
        assert!(!quit.active());
        quit.answer(1, Choice::StopAndQuit).unwrap();
        assert_eq!(script.exits.load(Ordering::SeqCst), 0);
        // The next Quit asks again with a new request id.
        quit.request_blocking();
        assert!(script.log().last().unwrap().starts_with("ask 2"));
    }

    #[test]
    fn a_failed_stop_keeps_silo_open_and_explains() {
        let script = Script::new(Ok(vec!["dev"]));
        *script.stop_error.lock().unwrap() = Some("dev would not stop".into());
        let quit = quit(&script);
        quit.request_blocking();
        quit.answer(1, Choice::StopAndQuit).unwrap();
        quit.begin_flush().unwrap();
        quit.complete_flush();
        let log = script.log();
        assert!(log
            .iter()
            .any(|entry| entry.starts_with("explain Silo stayed open")));
        assert!(!log.contains(&"exit".to_string()));
        assert!(!quit.active());
        assert!(quit.accepts_terminate_request());
    }

    #[test]
    fn a_failed_settings_save_keeps_silo_open_and_stops_nothing() {
        let script = Script::new(Ok(vec!["dev"]));
        *script.flush_error.lock().unwrap() = Some("disk full".into());
        let quit = quit(&script);
        quit.request_blocking();
        quit.answer(1, Choice::StopAndQuit).unwrap();
        quit.complete_flush();
        let log = script.log();
        assert!(log
            .iter()
            .any(|entry| entry.starts_with("explain Silo could not save")));
        assert!(!log.contains(&"stop".to_string()));
        assert_eq!(script.exits.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn the_native_fallback_finishes_only_when_the_webview_never_acknowledged() {
        let script = Script::new(Ok(vec![]));
        let quit = Quit::with_fallback(script.clone(), Duration::from_millis(50));
        quit.request_blocking();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(script.log().last().unwrap(), "exit");

        let script = Script::new(Ok(vec![]));
        let quit = Quit::with_fallback(script.clone(), Duration::from_millis(50));
        quit.request_blocking();
        quit.begin_flush().unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(script.exits.load(Ordering::SeqCst), 0);
        quit.complete_flush();
        assert_eq!(script.exits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_unreadable_computer_list_prompts_without_names_and_no_prompt_means_keep_running() {
        let script = Script::new(Err("engine unavailable"));
        let quit = quit(&script);
        quit.request_blocking();
        assert_eq!(script.log(), ["ask 1 []"]);

        let script = Script::new(Ok(vec!["dev"]));
        let quit = Quit::with_fallback(script.clone(), Duration::from_secs(30));
        // The main window never opted in to the prompt.
        quit.request_blocking();
        assert!(!script.log().iter().any(|entry| entry.starts_with("ask")));
        assert!(quit.active());
        assert!(!script.log().contains(&"stop".to_string()));
    }

    #[test]
    fn a_session_end_never_prompts_never_stops_and_exits_even_when_saving_fails() {
        let script = Script::new(Ok(vec!["dev"]));
        *script.flush_error.lock().unwrap() = Some("disk full".into());
        let quit = quit(&script);
        quit.end_session(Duration::from_secs(30));
        assert!(!script.log().iter().any(|entry| entry.starts_with("ask")));
        quit.begin_flush().unwrap();
        quit.complete_flush();
        assert!(!script.log().contains(&"stop".to_string()));
        assert_eq!(script.log().last().unwrap(), "exit");
    }

    #[test]
    fn a_session_end_deadline_forces_the_exit() {
        let script = Script::new(Ok(vec![]));
        let quit = Quit::with_fallback(script.clone(), Duration::from_secs(30));
        quit.end_session(Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(300));
        assert!(script.exits.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn the_flush_is_acknowledged_once_and_only_during_quit() {
        let script = Script::new(Ok(vec![]));
        let quit = quit(&script);
        assert!(quit.begin_flush().is_err());
        quit.request_blocking();
        quit.begin_flush().unwrap();
        assert!(quit.begin_flush().is_err());
    }

    #[test]
    fn a_choice_made_in_the_tray_skips_the_prompt() {
        let script = Script::new(Ok(vec!["dev"]));
        let quit = quit(&script);
        quit.quit_chosen(Choice::StopAndQuit);
        quit.begin_flush().unwrap();
        quit.complete_flush();
        let log = script.log();
        assert!(!log.iter().any(|entry| entry.starts_with("ask")));
        assert_eq!(&log[log.len() - 3..], ["settings", "stop", "exit"]);
    }

    #[test]
    fn choices_parse_from_the_webview_words() {
        assert_eq!(Choice::parse("keep"), Some(Choice::KeepRunning));
        assert_eq!(Choice::parse("stop"), Some(Choice::StopAndQuit));
        assert_eq!(Choice::parse("cancel"), Some(Choice::Cancel));
        assert_eq!(Choice::parse("true"), None);
    }
}
