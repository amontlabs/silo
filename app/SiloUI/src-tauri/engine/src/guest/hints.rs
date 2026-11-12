//! A small queue of folder hints that never grows past a fixed size.
//!
//! A hint only says "this folder changed, read it again", so a hint that is already waiting
//! absorbs any later one for the same folder. The queue holds each folder at most once and
//! at most [`HINT_QUEUE_CAPACITY`] folders, whoever fills it and however slowly it is read.
use std::{
    collections::VecDeque,
    sync::{Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
};

/// The most distinct folders waiting at once; a watch covers at most 32.
pub(crate) const HINT_QUEUE_CAPACITY: usize = 64;

#[derive(Default)]
struct State {
    waiting: VecDeque<String>,
    closed: bool,
}

#[derive(Default)]
pub(crate) struct HintQueue {
    state: Mutex<State>,
    arrived: Condvar,
}

impl HintQueue {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Queues `folder` unless it is already waiting or the queue is full. Never blocks.
    pub(crate) fn push(&self, folder: &str) {
        let mut state = self.lock();
        if state.closed
            || state.waiting.len() >= HINT_QUEUE_CAPACITY
            || state.waiting.iter().any(|waiting| waiting == folder)
        {
            return;
        }
        state.waiting.push_back(folder.to_owned());
        self.arrived.notify_one();
    }

    /// Ends the queue: whatever waits is dropped, readers return at once and nothing more is
    /// accepted.
    pub(crate) fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        state.waiting.clear();
        drop(state);
        self.arrived.notify_all();
    }

    /// The oldest waiting folder, waiting up to `timeout` for one. `Err(false)` is a timeout,
    /// `Err(true)` means the queue is closed and empty.
    pub(crate) fn recv_timeout(&self, timeout: Duration) -> Result<String, bool> {
        let deadline = Instant::now() + timeout;
        let mut state = self.lock();
        loop {
            if let Some(folder) = state.waiting.pop_front() {
                return Ok(folder);
            }
            if state.closed {
                return Err(true);
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return Err(false);
            };
            state = self
                .arrived
                .wait_timeout(state, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }

    /// Takes every folder waiting now.
    pub(crate) fn drain(&self) -> Vec<String> {
        self.lock().waiting.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_waits_once_and_the_queue_never_outgrows_its_capacity() {
        let queue = HintQueue::default();
        for _ in 0..1000 {
            queue.push("/a");
            queue.push("/b");
        }
        for index in 0..10_000 {
            queue.push(&format!("/folder-{index}"));
        }
        let waiting = queue.drain();
        assert_eq!(waiting.len(), HINT_QUEUE_CAPACITY);
        assert_eq!(&waiting[..2], ["/a", "/b"]);
        queue.push("/a");
        assert_eq!(queue.drain(), ["/a"]);
    }

    #[test]
    fn closing_wakes_a_reader_and_refuses_more() {
        let queue = std::sync::Arc::new(HintQueue::default());
        let reader = {
            let queue = queue.clone();
            std::thread::spawn(move || queue.recv_timeout(Duration::from_secs(30)))
        };
        std::thread::sleep(Duration::from_millis(50));
        queue.close();
        assert_eq!(reader.join().unwrap(), Err(true));
        queue.push("/a");
        assert!(queue.drain().is_empty());
        assert_eq!(queue.recv_timeout(Duration::ZERO), Err(true));
    }

    #[test]
    fn a_quiet_queue_times_out() {
        let queue = HintQueue::default();
        assert_eq!(queue.recv_timeout(Duration::from_millis(20)), Err(false));
        queue.push("/a");
        assert_eq!(queue.recv_timeout(Duration::ZERO), Ok("/a".to_owned()));
    }
}
