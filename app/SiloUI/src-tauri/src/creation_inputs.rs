//! What creating a computer waits for before it takes the device-wide operation gate:
//! the VM image import and the ChatGPT for Linux download, both of which run in the
//! background. Waiting here, not under the gate, keeps lifecycle operations and Quit
//! from queueing behind a download that can take minutes.
use crate::runtime::RuntimePaths;
use std::collections::HashSet;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const POLL: Duration = Duration::from_millis(250);

/// What a creation needs before it can finish everything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Needs {
    /// Ids of the new computers.
    pub(crate) computers: Vec<String>,
    /// Ids of the new computers that get built-in computer use, so need the ChatGPT app.
    pub(crate) computer_use: Vec<String>,
}

impl Needs {
    pub(crate) fn is_empty(&self) -> bool {
        self.computers.is_empty()
    }
}

/// The ChatGPT app as seen by a waiting creation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ChatGpt {
    Pending,
    Downloading { received: u64, total: u64 },
    Verifying,
    Extracting,
    Ready,
    Failed { reason: String },
}

/// One step shown while waiting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Image,
    ChatGpt(ChatGpt),
}

/// The outside world of a wait, replaceable in tests.
pub(crate) trait Inputs: Send + Sync {
    /// Blocks until the VM image is imported.
    fn ensure_image(&self) -> Result<(), String>;
    fn chatgpt(&self) -> ChatGpt;
    /// Makes sure the app download runs (a no-op while it already does).
    fn start_chatgpt(&self);
    /// Silo is quitting: the wait ends.
    fn quitting(&self) -> bool;
    fn show(&self, step: &Step);
    /// The user chose to finish creation without computer use.
    fn skipped(&self) -> bool;
}

/// Builds the host-side inputs for one creation wait. The UI adapter installs this once at
/// startup; the wait itself and its state stay independent of Tauri.
pub(crate) trait InputsProvider: Send + Sync + 'static {
    fn create(&self, paths: &RuntimePaths, request_id: &str) -> Arc<dyn Inputs>;
}

static INPUTS_PROVIDER: OnceLock<Arc<dyn InputsProvider>> = OnceLock::new();

/// Installs the host adapter once at app startup.
pub(crate) fn install_provider(provider: Arc<dyn InputsProvider>) -> Result<(), String> {
    INPUTS_PROVIDER
        .set(provider)
        .map_err(|_| "Creation inputs are already installed".to_owned())
}

/// What the wait decided about computer use for the new computers.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Ready,
    /// The user chose to finish without waiting for the ChatGPT app.
    WithoutComputerUse,
}

const QUITTING: &str =
    "Silo is quitting and stopping its local computers. Wait for shutdown to finish.";

/// Waits for the image, then the ChatGPT app. Holds no operation gate and checks
/// `quitting` at every poll, so Quit ends it within a poll interval.
pub(crate) fn wait(inputs: &Arc<dyn Inputs>, needs: &Needs) -> Result<Outcome, String> {
    wait_polling(inputs, needs, POLL)
}

fn wait_polling(
    inputs: &Arc<dyn Inputs>,
    needs: &Needs,
    poll: Duration,
) -> Result<Outcome, String> {
    if needs.is_empty() {
        return Ok(Outcome::Ready);
    }
    wait_for_image(inputs, poll)?;
    if needs.computer_use.is_empty() {
        return Ok(Outcome::Ready);
    }
    inputs.start_chatgpt();
    let mut shown: Option<ChatGpt> = None;
    loop {
        if inputs.quitting() {
            return Err(QUITTING.into());
        }
        if inputs.skipped() {
            return Ok(Outcome::WithoutComputerUse);
        }
        let status = inputs.chatgpt();
        if status == ChatGpt::Ready {
            return Ok(Outcome::Ready);
        }
        if shown.as_ref() != Some(&status) {
            inputs.show(&Step::ChatGpt(status.clone()));
            shown = Some(status);
        }
        std::thread::sleep(poll);
    }
}

/// The import blocks and cannot be interrupted, so it runs on its own thread: Quit ends the
/// wait, and the import finishes (or is dropped with the process) on its own.
fn wait_for_image(inputs: &Arc<dyn Inputs>, poll: Duration) -> Result<(), String> {
    let (done, result) = mpsc::channel();
    let importer = Arc::clone(inputs);
    let spawned = std::thread::Builder::new()
        .name("creation-image-wait".into())
        .spawn(move || {
            let _ = done.send(importer.ensure_image());
        });
    if spawned.is_err() {
        return Err("Silo could not wait for the VM image. Try again.".into());
    }
    inputs.show(&Step::Image);
    loop {
        match result.recv_timeout(poll) {
            Ok(outcome) => return outcome,
            Err(RecvTimeoutError::Timeout) if inputs.quitting() => return Err(QUITTING.into()),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err("VM image preparation stopped unexpectedly. Try again.".into())
            }
        }
    }
}

// ------------------------------------------------------------ the real inputs

static SKIPPED_REQUESTS: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static WITHOUT_COMPUTER_USE: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn with_set<T>(
    slot: &Mutex<Option<HashSet<String>>>,
    work: impl FnOnce(&mut HashSet<String>) -> T,
) -> T {
    let mut guard = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    work(guard.get_or_insert_with(HashSet::new))
}

/// "Finish without computer use" for the creation of `request_id`.
pub(crate) fn skip(request_id: &str) {
    with_set(&SKIPPED_REQUESTS, |set| {
        set.insert(request_id.to_owned());
    });
}

/// Whether the user asked this creation to finish without computer use.
pub(crate) fn was_skipped(request_id: &str) -> bool {
    with_set(&SKIPPED_REQUESTS, |set| set.contains(request_id))
}

/// Records that these new computers finish creation without the computer-use setup.
pub(crate) fn exclude_computer_use(computer_ids: &[String]) {
    with_set(&WITHOUT_COMPUTER_USE, |set| {
        set.extend(computer_ids.iter().cloned());
    });
}

/// Whether creation of `computer_id` skips the computer-use setup; consumed by the creation.
pub(crate) fn take_without_computer_use(computer_id: &str) -> bool {
    with_set(&WITHOUT_COMPUTER_USE, |set| set.remove(computer_id))
}

/// Waits for everything `needs` before the creation takes its operation gate. When the user
/// finishes without computer use, the new computers are recorded so their creation skips it.
pub(crate) fn wait_before_gate(
    paths: &RuntimePaths,
    request_id: &str,
    needs: &Needs,
) -> Result<(), String> {
    let outcome = if needs.is_empty() {
        Ok(Outcome::Ready)
    } else {
        match INPUTS_PROVIDER.get() {
            Some(provider) => wait(&provider.create(paths, request_id), needs),
            None => Err("Creation inputs are not available yet. Try again.".to_owned()),
        }
    };
    with_set(&SKIPPED_REQUESTS, |set| {
        set.remove(request_id);
    });
    if outcome? == Outcome::WithoutComputerUse {
        exclude_computer_use(&needs.computer_use);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct Fake {
        image_ms: u64,
        statuses: Mutex<Vec<ChatGpt>>,
        quit: AtomicBool,
        skip: AtomicBool,
        gate_held_during_wait: AtomicBool,
        shown: Mutex<Vec<Step>>,
        started: AtomicUsize,
    }

    impl Fake {
        fn new(image_ms: u64, statuses: Vec<ChatGpt>) -> Arc<Self> {
            Arc::new(Self {
                image_ms,
                statuses: Mutex::new(statuses),
                quit: AtomicBool::new(false),
                skip: AtomicBool::new(false),
                gate_held_during_wait: AtomicBool::new(false),
                shown: Mutex::new(Vec::new()),
                started: AtomicUsize::new(0),
            })
        }
        fn note_gate(&self) {
            if runtime::operation_gate::held() {
                self.gate_held_during_wait.store(true, Ordering::SeqCst);
            }
        }
    }

    impl Inputs for Fake {
        fn ensure_image(&self) -> Result<(), String> {
            self.note_gate();
            std::thread::sleep(Duration::from_millis(self.image_ms));
            Ok(())
        }
        fn chatgpt(&self) -> ChatGpt {
            self.note_gate();
            let mut statuses = self.statuses.lock().unwrap();
            if statuses.len() > 1 {
                statuses.remove(0)
            } else {
                statuses[0].clone()
            }
        }
        fn start_chatgpt(&self) {
            self.started.fetch_add(1, Ordering::SeqCst);
        }
        fn quitting(&self) -> bool {
            self.quit.load(Ordering::SeqCst)
        }
        fn show(&self, step: &Step) {
            self.note_gate();
            self.shown.lock().unwrap().push(step.clone());
        }
        fn skipped(&self) -> bool {
            self.skip.load(Ordering::SeqCst)
        }
    }

    fn needs() -> Needs {
        Needs {
            computers: vec!["a".into()],
            computer_use: vec!["a".into()],
        }
    }

    const FAST: Duration = Duration::from_millis(5);

    #[test]
    fn nothing_to_create_waits_for_nothing() {
        let fake = Fake::new(0, vec![ChatGpt::Pending]);
        let inputs: Arc<dyn Inputs> = fake.clone();
        assert_eq!(
            wait_polling(&inputs, &Needs::default(), FAST),
            Ok(Outcome::Ready)
        );
        assert!(fake.shown.lock().unwrap().is_empty());
    }

    #[test]
    fn waits_for_the_image_then_the_download_without_the_gate() {
        let fake = Fake::new(
            30,
            vec![
                ChatGpt::Pending,
                ChatGpt::Downloading {
                    received: 1,
                    total: 4,
                },
                ChatGpt::Extracting,
                ChatGpt::Ready,
            ],
        );
        let inputs: Arc<dyn Inputs> = fake.clone();
        assert_eq!(wait_polling(&inputs, &needs(), FAST), Ok(Outcome::Ready));
        assert!(!fake.gate_held_during_wait.load(Ordering::SeqCst));
        assert!(!runtime::operation_gate::held());
        assert_eq!(fake.started.load(Ordering::SeqCst), 1);
        let shown = fake.shown.lock().unwrap().clone();
        assert_eq!(shown[0], Step::Image);
        assert_eq!(shown[1], Step::ChatGpt(ChatGpt::Pending));
        assert_eq!(shown.len(), 4);
    }

    #[test]
    fn a_creation_without_computer_use_skips_the_download() {
        let fake = Fake::new(0, vec![ChatGpt::Pending]);
        let inputs: Arc<dyn Inputs> = fake.clone();
        let needs = Needs {
            computers: vec!["a".into()],
            computer_use: Vec::new(),
        };
        assert_eq!(wait_polling(&inputs, &needs, FAST), Ok(Outcome::Ready));
        assert_eq!(fake.started.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn quit_during_the_image_import_ends_the_wait() {
        let fake = Fake::new(60_000, vec![ChatGpt::Pending]);
        fake.quit.store(true, Ordering::SeqCst);
        let inputs: Arc<dyn Inputs> = fake.clone();
        let started = std::time::Instant::now();
        assert_eq!(
            wait_polling(&inputs, &needs(), FAST),
            Err(QUITTING.to_owned())
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn quit_during_the_download_ends_the_wait() {
        let fake = Fake::new(
            0,
            vec![ChatGpt::Failed {
                reason: "offline".into(),
            }],
        );
        let inputs: Arc<dyn Inputs> = fake.clone();
        let quitter = fake.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            quitter.quit.store(true, Ordering::SeqCst);
        });
        assert_eq!(
            wait_polling(&inputs, &needs(), FAST),
            Err(QUITTING.to_owned())
        );
        handle.join().unwrap();
        assert!(fake
            .shown
            .lock()
            .unwrap()
            .contains(&Step::ChatGpt(ChatGpt::Failed {
                reason: "offline".into()
            })));
    }

    #[test]
    fn the_user_can_finish_without_computer_use() {
        let fake = Fake::new(
            0,
            vec![ChatGpt::Failed {
                reason: "offline".into(),
            }],
        );
        let inputs: Arc<dyn Inputs> = fake.clone();
        let skipper = fake.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            skipper.skip.store(true, Ordering::SeqCst);
        });
        assert_eq!(
            wait_polling(&inputs, &needs(), FAST),
            Ok(Outcome::WithoutComputerUse)
        );
        handle.join().unwrap();
    }

    #[test]
    fn exclusion_is_consumed_once() {
        exclude_computer_use(&["unit-test-computer".into()]);
        assert!(take_without_computer_use("unit-test-computer"));
        assert!(!take_without_computer_use("unit-test-computer"));
    }
}
