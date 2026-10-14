//! The update hold: one thread that owns admission for the whole engine-update transaction.
//!
//! The guards that keep ordinary work out (the update admission lock, the GitHub and secrets
//! update locks, the device operation gate) are bound to the thread that took them, and must
//! outlive the `engine.update.prepare` request that started the transaction. A dedicated
//! thread acquires them, then serves prepare, abort, release and recovery commands until the
//! transaction ends or the engine exits.
use std::{
    sync::{mpsc, Arc},
    thread,
    time::Instant,
};

use silo_api::jobs::JobId;

use super::{
    adapter::{RunningComputer, SaveAdapter},
    journal::Store,
    transaction::{Coordinator, PrepareRequest, Recovery},
};

/// What a platform keeps held while the transaction runs. Dropped on the hold thread.
pub trait HeldGuards {
    /// Shows the step the transaction is on in the operation queue.
    fn relabel(&self, _label: &str) {}
}

/// The engine's host services for updates and shutdown. The real implementation lives in
/// the process that hosts the engine; tests use fakes.
pub trait Platform: Send + Sync {
    /// Adapters for the computer kinds this engine runs, created for one command.
    fn adapters(&self) -> Vec<Box<dyn SaveAdapter>>;

    /// Called on the hold thread. Closes admission to new work and takes the update locks.
    /// `job` is the job whose gate entry shows the hold in the queue, when there is one.
    fn acquire_guards(&self, job: Option<&JobId>) -> Result<Box<dyn HeldGuards>, String>;

    /// Closes admission everywhere for a shutdown that is not for an update.
    fn begin_shutdown(&self) {}

    /// Stops this device's computers for a shutdown that was asked to stop them.
    fn stop_computers(&self, deadline: Option<Instant>) -> Result<(), String>;

    /// Saves engine settings before exit.
    fn flush_settings(&self) -> Result<(), String> {
        Ok(())
    }

    /// Closes engine-owned tunnels and viewer connections before exit.
    fn close_connections(&self) {}

    /// The macOS build, recorded in the journal to compare after the update.
    fn host_build(&self) -> String {
        String::new()
    }

    /// Reopens the admission that a failed shutdown closed.
    fn reopen_admission(&self) {}

    /// Where engine releases come from. Without a configured feed nothing is ever found.
    fn update_provider(&self) -> Box<dyn super::updater::Provider> {
        Box::new(super::updater::NoFeed)
    }

    /// Whether an update could be prepared now: the names of the computers it would save
    /// and resume, or the reason it cannot start.
    fn update_readiness(&self) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }
}

type Reply<T> = mpsc::Sender<T>;

enum Command {
    Prepare(PrepareRequest, Reply<Result<Vec<RunningComputer>, String>>),
    Abort(Reply<Result<(), String>>),
    Release(Reply<Result<(), String>>),
    Recover(Reply<Result<Option<Recovery>, String>>),
    Prepared(Reply<Result<bool, String>>),
    OwnsLiveMachines(Reply<bool>),
    Stop,
}

/// A running hold. Dropping it ends the thread, which releases every guard.
pub struct Hold {
    commands: mpsc::Sender<Command>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Hold {
    /// Starts the hold thread and waits until it owns every guard.
    pub fn start(
        platform: Arc<dyn Platform>,
        store: Store,
        job: Option<JobId>,
    ) -> Result<Self, String> {
        let (commands, receiver) = mpsc::channel::<Command>();
        let (ready_sender, ready) = mpsc::channel::<Result<(), String>>();
        let thread = thread::Builder::new()
            .name("silo-update-hold".into())
            .spawn(move || {
                let guards = match platform.acquire_guards(job.as_ref()) {
                    Ok(guards) => {
                        let _ = ready_sender.send(Ok(()));
                        guards
                    }
                    Err(error) => {
                        let _ = ready_sender.send(Err(error));
                        return;
                    }
                };
                let adapters = platform.adapters();
                let coordinator = Coordinator {
                    store: &store,
                    adapters: &adapters,
                };
                while let Ok(command) = receiver.recv() {
                    // The computers an unresolved journal names stay reserved after every
                    // step that may change which ones those are.
                    let sync = !matches!(
                        command,
                        Command::Stop | Command::Prepared(_) | Command::OwnsLiveMachines(_)
                    );
                    match command {
                        Command::Prepare(request, reply) => {
                            let request = PrepareRequest {
                                host_build: platform.host_build(),
                                ..request
                            };
                            let _ =
                                reply.send(coordinator.prepare(request, &|index, total, name| {
                                    guards.relabel(&format!("Saving {name} ({index} of {total})"));
                                }));
                        }
                        Command::Abort(reply) => {
                            let _ = reply.send(coordinator.abort());
                        }
                        Command::Release(reply) => {
                            guards.relabel("Releasing computers for the update");
                            let _ = reply.send(coordinator.release_for_update());
                        }
                        Command::Recover(reply) => {
                            guards.relabel("Resuming computers after the update");
                            let _ = reply.send(coordinator.recover());
                        }
                        Command::Prepared(reply) => {
                            let _ = reply.send(coordinator.prepared());
                        }
                        Command::OwnsLiveMachines(reply) => {
                            let _ = reply
                                .send(adapters.iter().any(|adapter| adapter.owns_live_machines()));
                        }
                        Command::Stop => break,
                    }
                    if sync {
                        super::reservation::sync(&store);
                    }
                }
                drop(guards);
            })
            .map_err(|error| format!("The update could not start: {error}"))?;
        match ready.recv() {
            Ok(Ok(())) => Ok(Self {
                commands,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err("The update coordinator stopped unexpectedly.".into())
            }
        }
    }

    fn call<T>(&self, command: impl FnOnce(Reply<T>) -> Command) -> Result<T, String> {
        let (reply, answer) = mpsc::channel();
        self.commands
            .send(command(reply))
            .map_err(|_| "The update coordinator stopped unexpectedly.".to_owned())?;
        answer
            .recv()
            .map_err(|_| "The update coordinator stopped unexpectedly.".to_owned())
    }

    pub fn prepare(&self, request: PrepareRequest) -> Result<Vec<RunningComputer>, String> {
        self.call(|reply| Command::Prepare(request, reply))?
    }

    pub fn abort(&self) -> Result<(), String> {
        self.call(Command::Abort)?
    }

    pub fn release(&self) -> Result<(), String> {
        self.call(Command::Release)?
    }

    pub fn recover(&self) -> Result<Option<Recovery>, String> {
        self.call(Command::Recover)?
    }

    pub fn prepared(&self) -> Result<bool, String> {
        self.call(Command::Prepared)?
    }

    pub fn owns_live_machines(&self) -> bool {
        self.call(Command::OwnsLiveMachines).unwrap_or(true)
    }

    /// Ends the thread and releases admission.
    pub fn end(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        self.stop();
    }
}
