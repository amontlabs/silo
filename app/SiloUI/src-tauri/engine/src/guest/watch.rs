//! Folder change hints (`guest.files.watch`).
//!
//! A small helper in the guest compares each watched folder (names, inodes, sizes,
//! modification times) and prints the folder whenever it differs from the last look. That
//! coalesces bursts, needs no native notification API on either guest platform, and costs one
//! long-lived command instead of a command per poll across the channel. A hint never claims
//! the content is authoritative: clients re-read.
//!
//! The cost of a look is bounded: at most 4,096 names are read per folder, each folder is
//! held as one digest whatever its size, and the pause between looks grows with the time a
//! look took (at least 200 ms, at most 5 s). A folder with more entries is followed by its
//! own metadata only, which still reports additions and removals. The number of watches per
//! computer is bounded too.
//!
//! `start_watch` returns only once the helper has recorded every folder's baseline, so a
//! change made after the response is always reported.
use super::{
    exec::{self, perl_argv, terminate},
    process::{self, StreamKind},
    target::Target,
    GuestError,
};
use silo_api::jobs::ErrorCode;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

const MAX_WATCHED_FOLDERS: usize = 32;
/// Active watches one computer may have at once; each is a long-lived command in the guest.
#[cfg(not(test))]
const MAX_WATCHES_PER_COMPUTER: usize = 16;
#[cfg(test)]
const MAX_WATCHES_PER_COMPUTER: usize = 3;
const LIVENESS_POLL: Duration = Duration::from_millis(250);
/// How long the helper may take to record its baselines (a very large folder, a slow channel).
const READY_DEADLINE: Duration = Duration::from_secs(30);

fn active_watches() -> std::sync::MutexGuard<'static, HashMap<String, usize>> {
    static ACTIVE: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
    ACTIVE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// One of a computer's watch slots, given back when dropped.
struct Slot(String);

impl Slot {
    fn take(computer: &str) -> Result<Self, GuestError> {
        let mut active = active_watches();
        let count = active.entry(computer.to_owned()).or_default();
        if *count >= MAX_WATCHES_PER_COMPUTER {
            return Err(GuestError::new(
                ErrorCode::Busy,
                format!(
                    "This computer already has {MAX_WATCHES_PER_COMPUTER} folder watches. Stop one first."
                ),
            ));
        }
        *count += 1;
        Ok(Self(computer.to_owned()))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut active = active_watches();
        if let Some(count) = active.get_mut(&self.0) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                active.remove(&self.0);
            }
        }
    }
}

/// A running watch. It ends on its own when `still_wanted` turns false or the computer
/// stops; `stop` ends it at once. Dropping the handle does not stop it.
pub struct WatchHandle {
    stop: Arc<AtomicBool>,
}

impl WatchHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Starts watching `paths` (folders) and calls `emit` with the watched folder each time its
/// contents changed.
///
/// `still_wanted` is asked a few times a second; the watch ends (and its guest process is
/// stopped) when it returns false, when `stop` is called, or when the computer stops.
/// The first look only records the folders, so nothing is reported for what already exists;
/// this function returns after that look, so everything that changes afterwards is reported.
pub fn start_watch(
    target: &Target,
    paths: &[String],
    emit: Arc<dyn Fn(String) + Send + Sync>,
    still_wanted: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<WatchHandle, GuestError> {
    if paths.is_empty() || paths.len() > MAX_WATCHED_FOLDERS {
        return Err(GuestError::invalid(format!(
            "Invalid params: watch between 1 and {MAX_WATCHED_FOLDERS} folders."
        )));
    }
    if paths
        .iter()
        .any(|path| !path.starts_with('/') || path.contains('\0'))
    {
        return Err(GuestError::invalid(
            "Invalid params: folders must be absolute paths without NUL.",
        ));
    }
    target.ensure_running()?;
    let slot = Slot::take(&target.slot_key())?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let mut arguments = vec!["watch".to_owned()];
    arguments.extend(paths.iter().cloned());
    let argv = exec::supervised(&token, None, &perl_argv(exec::FILES, &arguments));
    let plan = target.plan(&argv, &[], false, false)?;
    let pending = Arc::new(Mutex::new(Vec::<u8>::new()));
    let (armed, ready) = mpsc::channel::<()>();
    let stderr_head = Arc::new(Mutex::new(Vec::<u8>::new()));
    let chunks = {
        let stderr_head = stderr_head.clone();
        Arc::new(move |kind: StreamKind, data: Vec<u8>| {
            if kind == StreamKind::Stderr {
                let mut head = stderr_head
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let room = 1024usize.saturating_sub(head.len());
                head.extend_from_slice(&data[..data.len().min(room)]);
                return;
            }
            let mut buffered = pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            buffered.extend_from_slice(&data);
            while let Some(end) = buffered.iter().position(|byte| *byte == 0) {
                let path: Vec<u8> = buffered.drain(..=end).collect();
                if path.len() == 1 {
                    // The empty record: every baseline is recorded.
                    let _ = armed.send(());
                } else {
                    emit(String::from_utf8_lossy(&path[..path.len() - 1]).into_owned());
                }
            }
        })
    };
    let mut streaming = process::spawn_streaming(&plan, false, chunks)?;
    let deadline = Instant::now() + READY_DEADLINE;
    let explain = |streaming: &mut process::Streaming, status: std::process::ExitStatus| {
        // Let the output readers deliver what the helper said, then explain.
        streaming.finish_output();
        let head = String::from_utf8_lossy(
            &stderr_head
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
        .trim()
        .to_owned();
        let code = status.code().unwrap_or(-1);
        match target.channel_error(code, &head) {
            Some(error) => error.message,
            None if head.is_empty() => "The folder watch could not start.".to_owned(),
            None => format!("The folder watch could not start: {head}"),
        }
    };
    let failure = loop {
        match ready.recv_timeout(Duration::from_millis(20)) {
            Ok(()) => break None,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // Both output streams ended without the baseline: the helper is gone.
                break Some(match streaming.child.wait() {
                    Ok(status) => explain(&mut streaming, status),
                    Err(_) => "The folder watch could not start.".to_owned(),
                });
            }
        }
        if let Ok(Some(status)) = streaming.child.try_wait() {
            break Some(explain(&mut streaming, status));
        }
        if Instant::now() >= deadline {
            break Some("The computer took too long to start the folder watch.".to_owned());
        }
    };
    if let Some(message) = failure {
        let _ = terminate(target, false, &token);
        let _ = streaming.child.kill();
        let _ = streaming.child.wait();
        return Err(GuestError::new(ErrorCode::Internal, message));
    }
    let stop = Arc::new(AtomicBool::new(false));
    let handle = WatchHandle { stop: stop.clone() };
    let target = target.clone();
    thread::spawn(move || {
        let _slot = slot;
        loop {
            if stop.load(Ordering::SeqCst) || !still_wanted() {
                let _ = terminate(&target, false, &token);
                let _ = streaming.child.kill();
                break;
            }
            match streaming.child.try_wait() {
                Ok(None) => thread::sleep(LIVENESS_POLL),
                // The computer stopped (or the helper ended): the watch ends with it.
                _ => break,
            }
        }
        let _ = streaming.child.wait();
        streaming.finish_output();
    });
    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guest::test_support;
    use silo_api::jobs::ErrorCode;
    use std::time::Instant;

    type Seen = Arc<Mutex<Vec<String>>>;

    fn collector() -> (Seen, Arc<dyn Fn(String) + Send + Sync>) {
        let seen: Seen = Arc::default();
        let sink = seen.clone();
        (seen, Arc::new(move |path| sink.lock().unwrap().push(path)))
    }

    fn wait_for(seen: &Seen, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while seen.lock().unwrap().len() < count {
            assert!(Instant::now() < deadline, "no change was reported");
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn a_watch_reports_changed_folders_and_ends_when_no_longer_wanted() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let folder = directory.path().join("forms");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("existing"), b"old").unwrap();
        let wanted = Arc::new(AtomicBool::new(true));
        let still_wanted = {
            let wanted = wanted.clone();
            Arc::new(move || wanted.load(Ordering::SeqCst))
        };
        let path = folder.to_string_lossy().into_owned();
        let (seen, emit) = collector();
        let handle = start_watch(&target, &[path.clone()], emit, still_wanted).unwrap();
        assert!(
            seen.lock().unwrap().is_empty(),
            "what already exists is not a change"
        );
        std::fs::write(folder.join("answer.json"), b"{}").unwrap();
        wait_for(&seen, 1);
        assert_eq!(seen.lock().unwrap()[0], path);
        // A rewrite in place with the same size is a change too.
        std::fs::write(folder.join("answer.json"), b"[]").unwrap();
        wait_for(&seen, 2);

        wanted.store(false, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(1500));
        let settled = seen.lock().unwrap().len();
        std::fs::write(folder.join("after"), b"x").unwrap();
        thread::sleep(Duration::from_millis(1000));
        assert_eq!(
            seen.lock().unwrap().len(),
            settled,
            "a finished watch reports nothing"
        );
        handle.stop();
    }

    #[test]
    fn stopping_the_handle_ends_the_watch() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let folder = directory.path().join("inbox");
        std::fs::create_dir(&folder).unwrap();
        let (seen, emit) = collector();
        let handle = start_watch(
            &target,
            &[folder.to_string_lossy().into_owned()],
            emit,
            Arc::new(|| true),
        )
        .unwrap();
        handle.stop();
        thread::sleep(Duration::from_millis(1500));
        std::fs::write(folder.join("late"), b"x").unwrap();
        thread::sleep(Duration::from_millis(1000));
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn watch_arguments_are_validated_and_a_stopped_computer_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let attempt = |target: &Target, paths: Vec<String>| {
            let (_, emit) = collector();
            start_watch(target, &paths, emit, Arc::new(|| false)).err()
        };
        assert!(attempt(&target, Vec::new()).is_some());
        assert!(attempt(&target, vec!["relative".into()]).is_some());
        assert!(attempt(
            &target,
            (0..33).map(|index| format!("/tmp/{index}")).collect()
        )
        .is_some());

        let paths = test_support::runtime(directory.path());
        crate::test_support::write_shell_script(
            &paths.executable,
            r#"case "$1" in inspect) printf '%s\n' '{"name":"dev","status":"Stopped","config":{"labels":{"silo.managed":"true"}}}';; esac"#,
        );
        let stopped =
            crate::guest::resolve(Ok(paths), directory.path(), test_support::COMPUTER_ID).unwrap();
        let error = attempt(&stopped, vec!["/tmp".into()]).unwrap();
        assert_eq!(error.code, ErrorCode::NotRunning);
    }

    #[test]
    fn a_change_made_right_after_the_response_is_never_missed() {
        // The helper has recorded its baseline before `start_watch` returns, so a write that
        // follows at once is a change. No pause between the response and the write.
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        for round in 0..5 {
            let folder = directory.path().join(format!("immediate-{round}"));
            std::fs::create_dir(&folder).unwrap();
            let (seen, emit) = collector();
            let handle = start_watch(
                &target,
                &[folder.to_string_lossy().into_owned()],
                emit,
                Arc::new(|| true),
            )
            .unwrap();
            std::fs::write(folder.join("first"), b"x").unwrap();
            wait_for(&seen, 1);
            handle.stop();
        }
    }

    #[test]
    fn renames_and_a_folder_created_later_are_reported() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let folder = directory.path().join("renames");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("old"), b"x").unwrap();
        let later = directory.path().join("later");
        let (seen, emit) = collector();
        let handle = start_watch(
            &target,
            &[
                folder.to_string_lossy().into_owned(),
                later.to_string_lossy().into_owned(),
            ],
            emit,
            Arc::new(|| true),
        )
        .unwrap();
        std::fs::rename(folder.join("old"), folder.join("new")).unwrap();
        wait_for(&seen, 1);
        assert_eq!(seen.lock().unwrap()[0], folder.to_string_lossy());
        std::fs::create_dir(&later).unwrap();
        wait_for(&seen, 2);
        assert!(seen
            .lock()
            .unwrap()
            .iter()
            .any(|path| path == later.to_string_lossy().as_ref()));
        handle.stop();
    }

    #[test]
    fn a_huge_folder_is_followed_by_its_own_metadata_at_bounded_cost() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let folder = directory.path().join("huge");
        std::fs::create_dir(&folder).unwrap();
        for index in 0..4200 {
            std::fs::write(folder.join(format!("f{index}")), b"").unwrap();
        }
        let (seen, emit) = collector();
        let handle = start_watch(
            &target,
            &[folder.to_string_lossy().into_owned()],
            emit,
            Arc::new(|| true),
        )
        .unwrap();
        // Over the entry limit the folder's own metadata still reports additions.
        std::fs::write(folder.join("added"), b"x").unwrap();
        wait_for(&seen, 1);
        handle.stop();
    }

    #[test]
    fn a_watch_that_cannot_start_says_why_and_gives_its_slot_back() {
        let directory = tempfile::tempdir().unwrap();
        let paths = test_support::runtime(directory.path());
        crate::test_support::write_shell_script(
            &paths.executable,
            r#"case "$1" in
  inspect) printf '%s\n' '{"name":"dev","status":"Running","config":{"labels":{"silo.managed":"true"}},"runtime_instance_id":"instance-1"}';;
  --silo-working-account-protocol) printf '1\n';;
  exec) echo "no perl here" >&2; exit 9;;
esac"#,
        );
        let target =
            crate::guest::resolve(Ok(paths), directory.path(), test_support::COMPUTER_ID).unwrap();
        for _ in 0..(MAX_WATCHES_PER_COMPUTER + 2) {
            let (_, emit) = collector();
            let error = start_watch(&target, &["/tmp".into()], emit, Arc::new(|| true))
                .err()
                .unwrap();
            assert!(error.message.contains("no perl here"), "{}", error.message);
            assert_eq!(error.code, ErrorCode::Internal);
        }
    }

    #[test]
    fn a_computer_has_a_bounded_number_of_watches() {
        let directory = tempfile::tempdir().unwrap();
        let target = test_support::target(directory.path());
        let folder = directory.path().join("shared");
        std::fs::create_dir(&folder).unwrap();
        let path = folder.to_string_lossy().into_owned();
        let mut handles = Vec::new();
        for _ in 0..MAX_WATCHES_PER_COMPUTER {
            let (_, emit) = collector();
            handles.push(start_watch(&target, &[path.clone()], emit, Arc::new(|| true)).unwrap());
        }
        let (_, emit) = collector();
        let error = start_watch(&target, &[path.clone()], emit, Arc::new(|| true))
            .err()
            .unwrap();
        assert_eq!(error.code, ErrorCode::Busy);
        // Stopping one frees its slot once its guest process is gone.
        handles.pop().unwrap().stop();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let (_, emit) = collector();
            match start_watch(&target, &[path.clone()], emit, Arc::new(|| true)) {
                Ok(handle) => {
                    handles.push(handle);
                    break;
                }
                Err(error) => {
                    assert_eq!(error.code, ErrorCode::Busy);
                    assert!(Instant::now() < deadline, "the slot was never freed");
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
        for handle in handles {
            handle.stop();
        }
    }
}
