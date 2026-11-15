//! Channels for tests: they keep their listeners bound instead of serving them.

use std::{
    collections::HashMap,
    net::TcpListener,
    sync::{Condvar, Mutex},
    time::Duration,
};

use super::Channels;

/// Channels that keep their listeners bound and record what they were asked.
#[derive(Default)]
pub(crate) struct Fake {
    /// port -> (computer, served)
    pub(super) ports: Mutex<HashMap<u16, (String, bool, TcpListener)>>,
    pub(super) ended: Mutex<Vec<String>>,
    pub(super) fail_serve: Mutex<bool>,
    /// The first serve waits up to this long for a second one.
    pub(super) first_waits: Mutex<Option<Duration>>,
    pub(super) serves: Mutex<usize>,
    pub(super) served_again: Condvar,
}

impl Channels for Fake {
    fn serve(&self, computer_id: &str, listener: TcpListener) -> Result<(), String> {
        if *self.fail_serve.lock().unwrap() {
            return Err("the channel could not be served".into());
        }
        let wait = self.first_waits.lock().unwrap().take();
        {
            let mut serves = self.serves.lock().unwrap();
            *serves += 1;
            self.served_again.notify_all();
            if let Some(wait) = wait {
                let _ = self
                    .served_again
                    .wait_timeout_while(serves, wait, |serves| *serves < 2)
                    .unwrap();
            }
        }
        let port = listener.local_addr().unwrap().port();
        let mut ports = self.ports.lock().unwrap();
        for (computer, served, _) in ports.values_mut() {
            if computer == computer_id {
                *served = false;
            }
        }
        ports.insert(port, (computer_id.to_owned(), true, listener));
        Ok(())
    }
    fn hold(&self, computer_id: &str, listener: TcpListener) -> Result<(), String> {
        let port = listener.local_addr().unwrap().port();
        self.ports
            .lock()
            .unwrap()
            .insert(port, (computer_id.to_owned(), false, listener));
        Ok(())
    }
    fn serve_held(&self, computer_id: &str, port: u16) -> bool {
        let mut ports = self.ports.lock().unwrap();
        match ports.get_mut(&port) {
            Some((computer, served, _)) if computer == computer_id => {
                *served = true;
                true
            }
            _ => false,
        }
    }
    fn close(&self, computer_id: &str) {
        for (computer, served, _) in self.ports.lock().unwrap().values_mut() {
            if computer == computer_id {
                *served = false;
            }
        }
        self.ended.lock().unwrap().push(computer_id.to_owned());
    }
    fn end_sessions(&self, computer_id: &str) {
        self.ended.lock().unwrap().push(computer_id.to_owned());
    }
    fn release(&self, port: u16) {
        self.ports.lock().unwrap().remove(&port);
    }
    fn serving(&self, computer_id: &str) -> Option<u16> {
        self.ports
            .lock()
            .unwrap()
            .iter()
            .find(|(_, (computer, served, _))| computer == computer_id && *served)
            .map(|(port, _)| *port)
    }
    fn bound(&self, port: u16) -> Option<String> {
        self.ports
            .lock()
            .unwrap()
            .get(&port)
            .map(|(computer, _, _)| computer.clone())
    }
}
