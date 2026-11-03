//! What creating a computer waits for before it takes the device-wide operation gate:
//! the VM image import, which runs in the background. Waiting here, not under the gate,
//! keeps lifecycle operations and Quit from queueing behind a download that can take
//! minutes.
use crate::runtime::RuntimePaths;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

const POLL: Duration = Duration::from_millis(250);

/// What a creation needs before it can finish everything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Needs {
    /// Ids of the new computers.
    pub(crate) computers: Vec<String>,
}

impl Needs {
    pub(crate) fn is_empty(&self) -> bool {
        self.computers.is_empty()
    }
}

/// One step shown while waiting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Image,
}

/// The outside world of a wait, replaceable in tests.
pub trait Inputs: Send + Sync {
    /// Blocks until the VM image is imported.
    fn ensure_image(&self) -> Result<(), String>;
    /// Silo is quitting: the wait ends.
    fn quitting(&self) -> bool;
    fn show(&self, step: &Step);
}

/// Builds the host-side inputs for one creation wait. The UI adapter installs this once at
/// startup; the wait itself and its state stay independent of the desktop host.
pub trait InputsProvider: Send + Sync + 'static {
    fn create(&self, paths: &RuntimePaths, request_id: &str) -> Arc<dyn Inputs>;
}

static INPUTS_PROVIDER: OnceLock<Arc<dyn InputsProvider>> = OnceLock::new();

/// Installs the host adapter once at app startup.
pub fn install_provider(provider: Arc<dyn InputsProvider>) -> Result<(), String> {
    INPUTS_PROVIDER
        .set(provider)
        .map_err(|_| "Creation inputs are already installed".to_owned())
}

const QUITTING: &str =
    "Silo is quitting and stopping its local computers. Wait for shutdown to finish.";

/// Waits for the image. Holds no operation gate and checks `quitting` at every poll, so Quit
/// ends it within a poll interval.
pub(crate) fn wait(inputs: &Arc<dyn Inputs>, needs: &Needs) -> Result<(), String> {
    wait_polling(inputs, needs, POLL)
}

fn wait_polling(inputs: &Arc<dyn Inputs>, needs: &Needs, poll: Duration) -> Result<(), String> {
    if needs.is_empty() {
        return Ok(());
    }
    wait_for_image(inputs, poll)
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

/// Waits for everything `needs` before the creation takes its operation gate.
pub(crate) fn wait_before_gate(
    paths: &RuntimePaths,
    request_id: &str,
    needs: &Needs,
) -> Result<(), String> {
    if needs.is_empty() {
        return Ok(());
    }
    match INPUTS_PROVIDER.get() {
        Some(provider) => wait(&provider.create(paths, request_id), needs),
        None => Err("Creation inputs are not available yet. Try again.".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    struct Fake {
        image_ms: u64,
        quit: AtomicBool,
        gate_held_during_wait: AtomicBool,
        shown: Mutex<Vec<Step>>,
    }

    impl Fake {
        fn new(image_ms: u64) -> Arc<Self> {
            Arc::new(Self {
                image_ms,
                quit: AtomicBool::new(false),
                gate_held_during_wait: AtomicBool::new(false),
                shown: Mutex::new(Vec::new()),
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
        fn quitting(&self) -> bool {
            self.quit.load(Ordering::SeqCst)
        }
        fn show(&self, step: &Step) {
            self.note_gate();
            self.shown.lock().unwrap().push(step.clone());
        }
    }

    fn needs() -> Needs {
        Needs {
            computers: vec!["a".into()],
        }
    }

    const FAST: Duration = Duration::from_millis(5);

    #[test]
    fn nothing_to_create_waits_for_nothing() {
        let fake = Fake::new(0);
        let inputs: Arc<dyn Inputs> = fake.clone();
        assert_eq!(wait_polling(&inputs, &Needs::default(), FAST), Ok(()));
        assert!(fake.shown.lock().unwrap().is_empty());
    }

    #[test]
    fn waits_for_the_image_without_the_gate() {
        let fake = Fake::new(30);
        let inputs: Arc<dyn Inputs> = fake.clone();
        assert_eq!(wait_polling(&inputs, &needs(), FAST), Ok(()));
        assert!(!fake.gate_held_during_wait.load(Ordering::SeqCst));
        assert!(!runtime::operation_gate::held());
        assert_eq!(*fake.shown.lock().unwrap(), vec![Step::Image]);
    }

    #[test]
    fn quit_during_the_image_import_ends_the_wait() {
        let fake = Fake::new(60_000);
        fake.quit.store(true, Ordering::SeqCst);
        let inputs: Arc<dyn Inputs> = fake.clone();
        let started = std::time::Instant::now();
        assert_eq!(
            wait_polling(&inputs, &needs(), FAST),
            Err(QUITTING.to_owned())
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
