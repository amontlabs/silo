//! Folder change hints (`guest.files.watch`).
//!
//! A small helper in the guest compares each watched folder's listing (names, inodes, sizes,
//! modification times) a few times a second and prints the folder whenever it differs from
//! the last look. That coalesces bursts, needs no native notification API on either guest
//! platform, and costs one long-lived command instead of a command per poll across the
//! channel. A hint never claims the content is authoritative: clients re-read.
use super::{
    exec::{self, perl_argv, terminate},
    process::{self, StreamKind},
    target::Target,
    GuestError,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

const MAX_WATCHED_FOLDERS: usize = 32;
const LIVENESS_POLL: Duration = Duration::from_millis(250);

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
/// The first look only records the folders, so nothing is reported for what already exists.
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
    let token = uuid::Uuid::new_v4().simple().to_string();
    let mut arguments = vec!["watch".to_owned()];
    arguments.extend(paths.iter().cloned());
    let argv = exec::supervised(&token, None, &perl_argv(exec::FILES, &arguments));
    let plan = target.plan(&argv, &[], false, false)?;
    let pending = Arc::new(Mutex::new(Vec::<u8>::new()));
    let chunks = Arc::new(move |kind: StreamKind, data: Vec<u8>| {
        if kind != StreamKind::Stdout {
            return;
        }
        let mut buffered = pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        buffered.extend_from_slice(&data);
        while let Some(end) = buffered.iter().position(|byte| *byte == 0) {
            let path: Vec<u8> = buffered.drain(..=end).collect();
            emit(String::from_utf8_lossy(&path[..path.len() - 1]).into_owned());
        }
    });
    let mut streaming = process::spawn_streaming(&plan, false, chunks)?;
    let stop = Arc::new(AtomicBool::new(false));
    let handle = WatchHandle { stop: stop.clone() };
    let target = target.clone();
    thread::spawn(move || {
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
        thread::sleep(Duration::from_millis(1200));
        assert!(
            seen.lock().unwrap().is_empty(),
            "what already exists is not a change"
        );
        std::fs::write(folder.join("answer.json"), b"{}").unwrap();
        wait_for(&seen, 1);
        assert_eq!(seen.lock().unwrap()[0], path);
        // A rewrite in place with the same size is a change too.
        thread::sleep(Duration::from_millis(300));
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
        thread::sleep(Duration::from_millis(1000));
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
}
