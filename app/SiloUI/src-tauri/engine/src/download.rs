//! Resumable HTTPS downloads and the file helpers around them, used by the background
//! preparation of the VM image.
//!
//! The downloader resumes a partial file with a `Range` request, never follows a
//! symlink at the download name, follows only HTTPS redirects and bounds how much
//! it writes. The caller verifies the finished file by checksum.

use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::Path,
    time::Duration,
};

/// The most a download of unknown size may write.
const UNKNOWN_SIZE_LIMIT: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub message: String,
    pub(crate) retryable: bool,
}

impl Error {
    pub(crate) fn retry(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: true,
        }
    }
    pub(crate) fn fatal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: false,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

fn effective_uid() -> u32 {
    unsafe { libc::geteuid() }
}

fn denied(why: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::PermissionDenied, why)
}


/// Restores owner access on directories so a tampered tree (for example modes
/// changed to 0500) can still be deleted.
pub(crate) fn make_tree_deletable(path: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.uid() != effective_uid() {
        return Ok(());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    for entry in fs::read_dir(path)?.flatten() {
        let _ = make_tree_deletable(&entry.path());
    }
    Ok(())

/// The length of an existing resumable download, 0 when there is none. Anything
/// that is not a plain single-link file of ours (a planted symlink, a hard link
/// to another file) is removed, never followed.
fn safe_part_len(part: &Path) -> std::io::Result<u64> {
    match fs::symlink_metadata(part) {
        Ok(meta) if meta.is_file() && meta.nlink() == 1 && meta.uid() == effective_uid() => {
            Ok(meta.len())
        }
        Ok(_) => fs::remove_file(part).map(|()| 0),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error),
    }
}

/// Opens the download file without following a link at its name. A fresh
/// download is created exclusively; a resume re-checks what it opened.
fn open_part(part: &Path, append: bool) -> std::io::Result<File> {
    if !append {
        match fs::remove_file(part) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        return OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(part);
    }
    let file = OpenOptions::new()
        .write(true)
        .append(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(part)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.nlink() != 1 || meta.uid() != effective_uid() {
        return Err(denied("download file is not a plain file"));
    }
    Ok(file)
}

/// Fetches `url` into `part` (resuming if the file already has a prefix) until
/// it holds `total` bytes, reporting the byte count. A `total` of 0 means the size is not
/// known: the download is complete when the server ends the stream (the caller verifies
/// it by checksum). Tests substitute this.
pub(crate) trait Downloader {
    fn fetch(
        &self,
        url: &str,
        part: &Path,
        total: u64,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), Error>;
}

pub(crate) struct HttpDownloader {
    attempts: u32,
    backoff: Duration,
    user_agent: String,
    /// Tests only: serve from a loopback HTTP server instead of HTTPS.
    #[cfg(test)]
    plain_http: bool,
}

/// What an unexpected HTTP status for a pinned download means. Only a confirmed removal
/// (404 or 410) is final; a refusal (401 or 403, typically a proxy, a filtering network
/// or a regional block that another network does not have) and every other answer are
/// retried with the usual backoff.
fn refusal(code: u16) -> Error {
    match code {
        404 | 410 => Error::fatal(format!(
            "The server no longer serves the pinned download (HTTP {code})."
        )),
        401 | 403 => Error::retry(format!(
            "The download server refused the request (HTTP {code}). A proxy, firewall or network filter may be blocking it. Silo tries again, including after you switch networks."
        )),
        code => Error::retry(format!("The download server answered HTTP {code}.")),
    }
}

impl Default for HttpDownloader {
    fn default() -> Self {
        let context = silo_engine::context::current();
        Self::for_app_version(
            context
                .as_ref()
                .map(|context| context.identity().version.as_str()),
        )
    }
}

impl HttpDownloader {
    fn for_app_version(version: Option<&str>) -> Self {
        Self {
            attempts: 5,
            backoff: Duration::from_secs(2),
            user_agent: user_agent_for_version(version),
            #[cfg(test)]
            plain_http: false,
        }
    }

    fn attempt(
        &self,
        client: &reqwest::blocking::Client,
        url: &str,
        part: &Path,
        total: u64,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), Error> {
        let disk = || {
            Error::retry(
                "Silo could not write the download. Check free disk space and retry.",
            )
        };
        let mut have = safe_part_len(part).map_err(|_| disk())?;
        if total > 0 && have > total {
            fs::remove_file(part).map_err(|_| disk())?;
            have = 0;
        }
        if total > 0 && have == total {
            progress(have);
            return Ok(());
        }
        let mut request = client.get(url);
        if have > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let mut response = request.send().map_err(|error| {
            if error.is_timeout() || error.is_connect() {
                Error::retry(
                    "Silo could not connect to the download server. A firewall or network filter may be holding Silo's connection.",
                )
            } else {
                Error::retry("Silo could not reach the download server.")
            }
        })?;
        let status = response.status();
        let append = match status.as_u16() {
            206 => true,
            200 => false,
            // An unknown size cannot tell a finished download from a stale one: keep it for
            // the caller's checksum, which discards it when it does not match.
            416 if total == 0 => return Ok(()),
            416 => {
                let _ = fs::remove_file(part);
                return Err(Error::retry("The download restarted."));
            }
            code => return Err(refusal(code)),
        };
        let mut file = open_part(part, append).map_err(|_| disk())?;
        let mut written = if append { have } else { 0 };
        let limit = if total == 0 {
            UNKNOWN_SIZE_LIMIT
        } else {
            total
        };
        let mut buffer = vec![0u8; 256 * 1024];
        loop {
            let count = response
                .read(&mut buffer)
                .map_err(|_| Error::retry("The download was interrupted."))?;
            if count == 0 {
                break;
            }
            if written + count as u64 > limit {
                drop(file);
                let _ = fs::remove_file(part);
                return Err(Error::retry(
                    "The download was larger than expected.",
                ));
            }
            file.write_all(&buffer[..count]).map_err(|_| disk())?;
            written += count as u64;
            progress(written);
        }
        file.sync_all().map_err(|_| disk())?;
        if total == 0 || written == total {
            Ok(())
        } else {
            Err(Error::retry("The download ended early."))
        }
    }
}

impl Downloader for HttpDownloader {
    fn fetch(
        &self,
        url: &str,
        part: &Path,
        total: u64,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), Error> {
        #[cfg(test)]
        let plain_http = self.plain_http;
        #[cfg(not(test))]
        let plain_http = false;
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            // The blocking client has no stall timeout; this bounds one attempt
            // (a large archive at 250 KB/s) and a retry resumes where it stopped.
            .timeout(Duration::from_secs(30 * 60))
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() < 5
                    && (attempt.url().scheme() == "https"
                        || (plain_http && attempt.url().scheme() == "http"))
                {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .https_only(!plain_http)
            .user_agent(self.user_agent.as_str())
            .build()
            .map_err(|_| Error::fatal("Silo could not start a secure download."))?;
        let mut last = Error::retry("The download failed.");
        for attempt in 0..self.attempts {
            if attempt > 0 {
                std::thread::sleep(self.backoff * 2u32.pow(attempt - 1));
            }
            match self.attempt(&client, url, part, total, progress) {
                Ok(()) => return Ok(()),
                Err(error) if error.retryable => last = error,
                Err(error) => return Err(error),
            }
        }
        Err(last)
    }
}

fn user_agent_for_version(version: Option<&str>) -> String {
    version.map_or_else(|| "Silo".to_owned(), |version| format!("Silo/{version}"))
}

pub(crate) fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
mod tests;
