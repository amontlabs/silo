//! Where `silo` finds the engine: inside a computer whose tools are on, the channel named by
//! `/etc/silo/endpoint.json` (or the file `SILO_ENDPOINT` names); everywhere else, the engine's
//! socket.

use std::{
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use silo_api::methods::guest_client::GUEST_ENDPOINT_PATH;

use crate::error::CliError;

/// Names the endpoint file to use instead of `/etc/silo/endpoint.json`.
pub const ENV: &str = "SILO_ENDPOINT";
/// The largest endpoint file read.
const MAX_FILE_BYTES: u64 = 4096;

/// A computer's channel, as its endpoint file gives it.
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub token: String,
    pub computer_id: String,
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Endpoint")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("token", &"<redacted>")
            .field("computer_id", &self.computer_id)
            .finish()
    }
}

impl Endpoint {
    pub fn load(path: &Path) -> Result<Self, CliError> {
        use std::io::Read;
        let mut text = String::new();
        std::fs::File::open(path)
            .and_then(|file| file.take(MAX_FILE_BYTES).read_to_string(&mut text))
            .map_err(|error| {
                CliError::new(format!(
                    "Cannot read {}: {error}. In a computer, Silo's tools are for the silo account (or root).",
                    path.display()
                ))
            })?;
        let endpoint: Self = serde_json::from_str(&text)
            .map_err(|error| CliError::new(format!("{} is not valid: {error}", path.display())))?;
        if endpoint.host.is_empty() || endpoint.port == 0 || endpoint.token.is_empty() {
            return Err(CliError::new(format!(
                "{} is not complete.",
                path.display()
            )));
        }
        Ok(endpoint)
    }
}

/// How this run reaches the engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transport {
    Socket(PathBuf),
    Channel(Endpoint),
}

/// Where the engine is. `--socket` always means the socket. Otherwise the file `SILO_ENDPOINT`
/// names, or the computer's own endpoint file when it exists, means the channel. Otherwise the
/// default socket, which `socket` resolves.
pub fn choose(
    socket_flag: Option<PathBuf>,
    endpoint_env: Option<OsString>,
    endpoint_file: &Path,
    socket: impl FnOnce(Option<PathBuf>) -> Result<PathBuf, CliError>,
) -> Result<Transport, CliError> {
    if socket_flag.is_some() {
        return socket(socket_flag).map(Transport::Socket);
    }
    if let Some(path) = endpoint_env.filter(|path| !path.is_empty()) {
        return Endpoint::load(Path::new(&path)).map(Transport::Channel);
    }
    if endpoint_file.exists() {
        return Endpoint::load(endpoint_file).map(Transport::Channel);
    }
    socket(None).map(Transport::Socket)
}

/// [`choose`] for this process: its flag, its environment and the fixed file.
pub fn for_this_process(
    socket_flag: Option<PathBuf>,
    allow_other: bool,
) -> Result<Transport, CliError> {
    choose(
        socket_flag,
        std::env::var_os(ENV),
        Path::new(GUEST_ENDPOINT_PATH),
        |flag| crate::engine::default_socket(flag, allow_other),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    const FILE: &str = r#"{"version":1,"host":"host.microsandbox.internal","port":41234,"token":"silo_gc_x","computerId":"c1"}"#;

    #[test]
    fn the_socket_flag_wins_then_the_named_file_then_the_computers_file_then_the_socket() {
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), "endpoint.json", FILE);
        let missing = dir.path().join("missing.json");
        let socket =
            |flag: Option<PathBuf>| Ok(flag.unwrap_or_else(|| PathBuf::from("/default.sock")));
        assert_eq!(
            choose(
                Some("/flag.sock".into()),
                Some(file.clone().into()),
                &file,
                socket
            )
            .unwrap(),
            Transport::Socket("/flag.sock".into())
        );
        let Transport::Channel(named) =
            choose(None, Some(file.clone().into()), &missing, socket).unwrap()
        else {
            panic!("the named file is the channel");
        };
        assert_eq!(named.port, 41234);
        assert_eq!(named.computer_id, "c1");
        assert!(matches!(
            choose(None, None, &file, socket).unwrap(),
            Transport::Channel(_)
        ));
        assert_eq!(
            choose(None, None, &missing, socket).unwrap(),
            Transport::Socket("/default.sock".into())
        );
        assert_eq!(
            choose(None, Some(OsString::new()), &missing, socket).unwrap(),
            Transport::Socket("/default.sock".into())
        );
    }

    #[test]
    fn a_named_file_that_cannot_be_read_is_an_error_not_the_socket() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.json");
        let bad = write(dir.path(), "bad.json", "{");
        let incomplete = write(
            dir.path(),
            "incomplete.json",
            r#"{"host":"h","port":0,"token":"t","computerId":"c"}"#,
        );
        let socket = |_: Option<PathBuf>| Ok(PathBuf::from("/default.sock"));
        for path in [&missing, &bad, &incomplete] {
            assert!(choose(None, Some(path.clone().into()), &missing, socket).is_err());
        }
    }

    #[test]
    fn the_token_is_never_printed() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = Endpoint::load(&write(dir.path(), "e.json", FILE)).unwrap();
        assert!(!format!("{endpoint:?}").contains("silo_gc_x"));
        assert!(!format!("{:?}", Transport::Channel(endpoint)).contains("silo_gc_x"));
    }
}
