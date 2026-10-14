//! Opt-in live regressions for the built-in desktop and computer use, run against the
//! real runtime with disposable homes under `SILO_TEST_TMP` (default /tmp) and `e2e-*` computers. They need the guest
//! image directory (`SILO_TEST_GUEST_IMAGE`, a directory with manifest.json and
//! image.tar.gz), a published ChatGPT folder (`SILO_TEST_PUBLISHED`, `<root>/published` of
//! a prepared app), a signed msb (`SILO_TEST_MSB`, `SILO_TEST_LIBKRUNFW`) and
//! `SILO_LIVE_TEST_CONFIRM`.
use crate::runtime::{self, ComputerConfiguration, RuntimePaths};
use serde_json::Value;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// One disposable runtime home with its own metadata and, for built-in computers, the published
/// ChatGPT folder registered for this thread.
pub(crate) struct Fixture {
    directory: Option<tempfile::TempDir>,
    pub(crate) paths: RuntimePaths,
    names: Vec<String>,
}

impl Fixture {
    /// `image` overrides `SILO_TEST_GUEST_IMAGE` (a directory with manifest.json and
    /// image.tar.gz), for example to create a pre-v4 computer.
    pub(crate) fn new(prefix: &str, image: Option<PathBuf>, with_published: bool) -> Self {
        crate::test_support::live::require_confirmation();
        let directory = tempfile::Builder::new()
            .prefix(prefix)
            .tempdir_in(crate::test_support::live::temp_root())
            .unwrap();
        let guest_image =
            image.unwrap_or_else(|| PathBuf::from(std::env::var("SILO_TEST_GUEST_IMAGE").unwrap()));
        if with_published {
            let published = crate::chatgpt_app::ensure_published_dir(
                PathBuf::from(std::env::var("SILO_TEST_PUBLISHED").unwrap())
                    .parent()
                    .unwrap(),
            )
            .unwrap();
            let version = crate::chatgpt_app::Lock::bundled()
                .unwrap()
                .directory_name(crate::chatgpt_app::DebArch::host().unwrap());
            crate::chatgpt_app::set_test_cache(Some(crate::chatgpt_app::Status::Ready {
                path: published.join(&version),
                version: "26.928.31416".into(),
            }));
            crate::computer_use::set_test_published_dir(Some(published));
        }
        let paths = RuntimePaths {
            guest_image,
            executable: PathBuf::from(std::env::var("SILO_TEST_MSB").unwrap()),
            library: PathBuf::from(std::env::var("SILO_TEST_LIBKRUNFW").unwrap()),
            home: directory.path().join("home"),
            storage_home: None,
            metadata: directory.path().join("computers.json"),
            volumes: directory.path().join("volumes"),
        };
        std::fs::create_dir_all(&paths.home).unwrap();
        std::fs::create_dir_all(&paths.volumes).unwrap();
        Self {
            directory: Some(directory),
            paths,
            names: Vec::new(),
        }
    }

    /// Stops `name` when the fixture drops, for computers created by other paths (forks).
    pub(crate) fn track(&mut self, name: &str) {
        self.names.push(name.to_owned());
    }

    pub(crate) fn create(&mut self, name: &str) -> ComputerConfiguration {
        self.names.push(name.to_owned());
        runtime::create_disposable_desktop_computer(&self.paths, name).unwrap()
    }

    pub(crate) fn computer_configuration(&self, name: &str) -> ComputerConfiguration {
        runtime::read_metadata(&self.paths.metadata)
            .unwrap()
            .computers
            .into_iter()
            .find(|configuration| configuration.name() == name)
            .unwrap()
    }

    pub(crate) fn exec(&self, name: &str, user: &str, script: &str) -> Result<String, String> {
        runtime::run_msb(
            &self.paths,
            &[
                "exec",
                name,
                "--no-start",
                "--user",
                user,
                "--workdir",
                "/",
                "--",
                "sh",
                "-c",
                script,
            ]
            .map(String::from),
            Duration::from_secs(300),
        )
        .map(|output| output.stdout)
        .map_err(|error| error.to_string())
    }

    pub(crate) fn stop(&self, name: &str) {
        runtime::run_msb(
            &self.paths,
            &["stop".into(), name.into()],
            Duration::from_secs(120),
        )
        .unwrap();
    }

    /// The desktop state Silo's own status path reports, computer use included.
    pub(crate) fn status(&self, name: &str) -> Value {
        crate::desktop::test_status(&self.paths, &self.computer_configuration(name)).unwrap()
    }

    /// Polls Silo's status until the session runs and computer use is ready.
    pub(crate) fn wait_ready(&self, name: &str, label: &str) -> (Value, Duration) {
        let started = Instant::now();
        let mut last = String::new();
        loop {
            let status = self.status(name);
            let state = status["computerUse"]["state"].as_str().unwrap_or("");
            let line = format!(
                "session {} stream {} computerUse {state} {}",
                status["sessionState"], status["streamState"], status["computerUse"]["reason"]
            );
            if line != last {
                eprintln!("[{:>4}s] {label}: {line}", started.elapsed().as_secs());
                last = line;
            }
            if state == "ready" && status["sessionState"] == "running" {
                return (status, started.elapsed());
            }
            if state == "failed" || started.elapsed() > Duration::from_secs(900) {
                eprintln!(
                    "{label} FAILED:\n{}",
                    self.exec(
                        name,
                        "root",
                        "tail -n 60 /var/log/silo-desktop.log; tail -n 40 /var/log/silo-computer-use.log; cat /var/lib/silo-computer-use/receipt.json",
                    )
                    .unwrap_or_default()
                );
                panic!("{label}: {last}");
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for name in &self.names {
            let _ = runtime::run_msb(
                &self.paths,
                &["stop".into(), name.clone()],
                Duration::from_secs(60),
            );
        }
        crate::computer_use::set_test_published_dir(None);
        crate::chatgpt_app::set_test_cache(None);
        // `SILO_LIVE_KEEP=1` leaves the (stopped) home behind for inspection after a failure.
        if std::env::var_os("SILO_LIVE_KEEP").is_some() {
            eprintln!("kept {}", self.paths.home.display());
            std::mem::forget(self.directory.take());
        }
    }
}

#[path = "computer_use_live/approval.rs"]
mod approval;
#[path = "computer_use_live/lcu.rs"]
mod lcu;
