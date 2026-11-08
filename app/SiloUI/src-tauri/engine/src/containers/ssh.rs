//! The SSH server of a container computer: `<runtime executable> ssh serve NAME ...`.
//!
//! A container has no `sshd`, as a MicroSandbox virtual machine has none: MicroSandbox's own
//! `msb ssh serve` is a host-side server (russh) that maps a shell, a command, a terminal and the
//! `sftp` subsystem onto its guest agent. The engine's callers (the editor's `ProxyCommand`, SSH
//! access listeners, Connections' remote guest connections) spawn that command with a fixed set of
//! flags, so this module answers the same command for a container, with the same server library:
//!
//! * authentication is the authorized-keys file the caller names (the editor's shared file by
//!   default), read at every attempt, public keys only, for the working account;
//! * the host key is `sandboxes/<name>/ssh/host_ed25519`, which the callers generate and pin;
//! * a shell, a command or the SFTP server runs as the working account through the engine's own
//!   exec path (`silo-ctr enter`, `crun exec`, the init shim, a process group the engine can end),
//!   with a terminal when the client asked for one;
//! * the listener prints the readiness line the callers wait for.
//!
//! MicroSandbox's server is bound to its sandbox and agent types and cannot take another backend;
//! a backend trait upstream would let both kinds share it (see `docs/v1/upstream`).

use super::args::SshServe;
use russh::keys::ssh_key::PublicKey;
use std::path::{Path, PathBuf};

/// What the listener prints, once bound, for the process that started it (`ssh_access`).
pub const READY_LINE: &str = "SILO_SSH_READY";
/// Printed (listener mode, for the owner) as sessions open and close.
pub const SESSION_OPEN_LINE: &str = "SILO_SSH_SESSION_OPEN";
pub const SESSION_CLOSED_LINE: &str = "SILO_SSH_SESSION_CLOSED";

/// The SFTP server of the guest image (`openssh-sftp-server`).
pub const SFTP_SERVER: &str = "/usr/lib/openssh/sftp-server";

/// Open channels one connection may have, and connections one server serves at once.
const MAX_CHANNELS: usize = 16;
const MAX_CONNECTIONS: usize = 32;
/// How long an idle connection may last unless the caller turned the timeout off.
const INACTIVITY: std::time::Duration = std::time::Duration::from_secs(600);
/// How long a connection has to log in, from the moment it is accepted, whatever the inactivity
/// timeout says (the caller may have turned that off for the editor's long-lived connections).
const LOGIN_GRACE: std::time::Duration = std::time::Duration::from_secs(30);
/// Connections that have not logged in yet, one server holds at once. Only a logged-in connection
/// counts against `MAX_CONNECTIONS`, so idle sockets cannot lock a person out.
const MAX_UNAUTHENTICATED: usize = 64;

/// What a client asked to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Shell,
    Command(String),
    Sftp,
}

impl Request {
    /// The command line inside the computer.
    pub fn argv(&self) -> Vec<String> {
        match self {
            Request::Shell => super::args::LOGIN_SHELL.map(str::to_owned).to_vec(),
            Request::Command(command) => {
                vec!["/bin/bash".into(), "-c".into(), command.clone()]
            }
            Request::Sftp => vec![SFTP_SERVER.into()],
        }
    }
}

/// Where a `direct-tcpip` channel (`ssh -L`, `ssh -D`, an editor's port forwarding) may connect:
/// the container's own loopback only, as `localhost` or a loopback address. Anything else,
/// including every address of the host's and of the container's network, is refused. The port is
/// 1..=65535. Returns the host as it is passed on to the connection.
pub fn loopback_destination(host: &str, port: u32) -> Option<(String, u16)> {
    let port = u16::try_from(port).ok().filter(|port| *port != 0)?;
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let loopback = bare.eq_ignore_ascii_case("localhost")
        || bare
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback());
    loopback.then(|| (bare.to_owned(), port))
}

/// The signal number for an SSH `signal` request; a name the server does not know is ignored.
pub fn signal_number(signal: &russh::Sig) -> Option<i32> {
    use russh::Sig;
    Some(match signal {
        Sig::ABRT => libc::SIGABRT,
        Sig::ALRM => libc::SIGALRM,
        Sig::FPE => libc::SIGFPE,
        Sig::HUP => libc::SIGHUP,
        Sig::ILL => libc::SIGILL,
        Sig::INT => libc::SIGINT,
        Sig::KILL => libc::SIGKILL,
        Sig::PIPE => libc::SIGPIPE,
        Sig::QUIT => libc::SIGQUIT,
        Sig::SEGV => libc::SIGSEGV,
        Sig::TERM => libc::SIGTERM,
        Sig::USR1 => libc::SIGUSR1,
        Sig::Custom(_) => return None,
    })
}

/// The refusal when `silo` in the computer's account files is not the working account. The
/// container's id mapping gives the engine's own user the id `AGENT_ID` and nobody else
/// (`containers::idmap`), so any other id would be a different, unmapped or privileged account.
pub fn check_working_identity(
    identity: &super::users::Identity,
    name: &str,
) -> Result<(), String> {
    use super::idmap::AGENT_ID;
    if identity.uid == AGENT_ID && identity.uid != 0 {
        Ok(())
    } else {
        Err(format!(
            "The account {} in {name} has the id {}, not {AGENT_ID}: SSH access is refused. Recreate the computer.",
            crate::working_account::USER,
            identity.uid
        ))
    }
}

/// How long the listener waits before it accepts again after `accept` failed `failures` times in a
/// row (out of file descriptors, say): 50 ms, doubling, at most 2 s.
pub fn accept_backoff(failures: u32) -> std::time::Duration {
    std::time::Duration::from_millis(50u64 << failures.saturating_sub(1).min(6)).min(
        std::time::Duration::from_secs(2),
    )
}

/// The public keys an authorized-keys file lists. Only plain `type base64 [comment]` lines count:
/// a line with options before the key (`command=`, `restrict`, `from=`) would need the options
/// honoured, so it is skipped rather than accepted without them.
pub fn parse_authorized_keys(text: &str) -> Vec<PublicKey> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter(|line| {
            line.split_whitespace().next().is_some_and(|kind| {
                kind.starts_with("ssh-")
                    || kind.starts_with("ecdsa-sha2-")
                    || kind.starts_with("sk-")
            })
        })
        .filter_map(|line| PublicKey::from_openssh(line).ok())
        .collect()
}

/// Whether `offered` is one of `keys` (the key itself; the comment does not matter).
pub fn is_authorized(keys: &[PublicKey], offered: &PublicKey) -> bool {
    keys.iter().any(|key| key.key_data() == offered.key_data())
}

/// The refusal when the computer is not the one the caller meant (a recreated computer has a new
/// machine identity, and access granted to the old one must not carry over).
pub fn check_machine_id(actual: Option<&str>, expected: Option<&str>) -> Result<(), String> {
    match expected {
        Some(expected) if actual != Some(expected) => {
            Err("sandbox identity changed; SSH access refused".into())
        }
        _ => Ok(()),
    }
}

/// A terminal type from the client, or `xterm` when it holds anything but name characters.
pub fn clean_term(term: &str) -> String {
    let fine = !term.is_empty()
        && term.len() <= 64
        && term
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'));
    if fine {
        term.to_owned()
    } else {
        "xterm".into()
    }
}

/// The authorized-keys file when the caller names none: the editor's shared one.
pub fn default_authorized_keys(home: &Path) -> PathBuf {
    home.join("ssh").join("authorized_keys")
}

pub fn host_key_path(home: &Path, name: &str) -> PathBuf {
    home.join("sandboxes")
        .join(name)
        .join("ssh")
        .join("host_ed25519")
}

#[cfg(target_os = "linux")]
pub use server::serve;

#[cfg(not(target_os = "linux"))]
pub fn serve(_: &crate::runtime::RuntimePaths, _: &SshServe) -> Result<i32, String> {
    Err("Container computers run only on a Linux host.".into())
}

#[cfg(target_os = "linux")]
mod server {
    use super::*;
    use russh::server::{
        Auth, ChannelOpenHandle, Handler, Msg, PublicKeyOffer, Session, VerifiedPublicKey,
    };
    use russh::{Channel, ChannelId, ChannelOpenFailure};
    use crate::containers::{
        args::Exec,
        hold::ExecGroup,
        layout::Layout,
        lifecycle,
        tools::Tools,
    };
    use crate::runtime::RuntimePaths;
    use bytes::Bytes;
    use russh::server::Handle;
    use std::collections::HashMap;
    use std::io::Write as _;
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::process::Stdio;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
    use tokio::sync::{mpsc, watch, Semaphore};
    use tokio::task::JoinHandle;

    /// What every connection of one server shares.
    #[derive(Clone)]
    struct Shared {
        paths: RuntimePaths,
        tools: Tools,
        name: String,
        authorized: PathBuf,
        /// Logged-in connections (`MAX_CONNECTIONS`).
        connections: Arc<Semaphore>,
        /// Connections that have not logged in yet (`MAX_UNAUTHENTICATED`).
        unauthenticated: Arc<Semaphore>,
    }

    /// The terminal a client asked for.
    #[derive(Clone, Debug)]
    struct Pty {
        term: String,
        cols: u16,
        rows: u16,
    }

    /// Most bytes a client may have sent to a command that has not read them yet. The session loop
    /// never waits for the command (it must keep answering the client's window updates, or a
    /// command that writes as much as it reads would stop the transfer in both directions), so
    /// what the command has not taken waits here, up to this much.
    const INPUT_QUEUE_LIMIT: usize = 32 * 1024 * 1024;

    /// The bytes waiting for a command's input.
    #[derive(Clone)]
    struct Input {
        sender: mpsc::UnboundedSender<Bytes>,
        queued: Arc<AtomicUsize>,
    }

    impl Input {
        fn new() -> (Self, mpsc::UnboundedReceiver<Bytes>) {
            let (sender, receiver) = mpsc::unbounded_channel();
            let queued = Arc::new(AtomicUsize::new(0));
            (Self { sender, queued }, receiver)
        }

        /// Queues `data` without waiting. `false` when the command is that far behind.
        fn push(&self, data: &[u8]) -> bool {
            if self.queued.load(Ordering::Relaxed).saturating_add(data.len()) > INPUT_QUEUE_LIMIT {
                return false;
            }
            self.queued.fetch_add(data.len(), Ordering::Relaxed);
            if self.sender.send(Bytes::copy_from_slice(data)).is_err() {
                // The command no longer reads: its input is discarded, as it always was.
                self.queued.fetch_sub(data.len(), Ordering::Relaxed);
            }
            true
        }
    }

    /// A task that ends when its owner lets go of it.
    struct AbortOnDrop(JoinHandle<()>);

    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    /// One session channel: what is running for it and how to reach it.
    #[derive(Default)]
    struct Chan {
        pty: Option<Pty>,
        input: Option<Input>,
        master: Option<Arc<std::fs::File>>,
        group: Option<Arc<ExecGroup>>,
        task: Option<JoinHandle<()>>,
    }

    impl Chan {
        fn end(&mut self) {
            self.input = None;
            if let Some(group) = self.group.take() {
                group.kill();
            }
            if let Some(task) = self.task.take() {
                task.abort();
            }
        }
    }

    impl Drop for Chan {
        fn drop(&mut self) {
            self.end();
        }
    }

    /// What a connection of the listener holds against the server's limits.
    struct Limits {
        /// Held until the connection logs in.
        pending: Option<tokio::sync::OwnedSemaphorePermit>,
        /// Held from the login to the end.
        active: Option<tokio::sync::OwnedSemaphorePermit>,
        authenticated: watch::Sender<bool>,
    }

    struct Connection {
        shared: Arc<Shared>,
        channels: HashMap<ChannelId, Chan>,
        /// Open `direct-tcpip` channels, each served by a task that ends with the channel.
        forwards: HashMap<ChannelId, AbortOnDrop>,
        /// `None` over standard input and output, where the caller is the one process that
        /// started the server.
        limits: Option<Limits>,
    }

    impl Connection {
        /// Called when the client has proved its key: the connection now counts against
        /// `MAX_CONNECTIONS` and no longer against the unauthenticated ones.
        fn admit(&mut self) -> Result<(), russh::Error> {
            let Some(limits) = self.limits.as_mut() else {
                return Ok(());
            };
            if limits.active.is_none() {
                let permit = self
                    .shared
                    .connections
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| russh::Error::Disconnect)?;
                limits.active = Some(permit);
            }
            limits.pending = None;
            let _ = limits.authenticated.send(true);
            Ok(())
        }

        fn allowed(&self, user: &str, offered: &PublicKey) -> bool {
            user == crate::working_account::USER
                && std::fs::read_to_string(&self.shared.authorized)
                    .is_ok_and(|text| is_authorized(&parse_authorized_keys(&text), offered))
        }

        async fn run(
            &mut self,
            session: &mut Session,
            channel: ChannelId,
            request: Request,
        ) -> Result<(), russh::Error> {
            let Some(chan) = self.channels.get_mut(&channel) else {
                return session.channel_failure(channel);
            };
            if chan.task.is_some() {
                return session.channel_failure(channel);
            }
            let pty = chan.pty.clone();
            match start(&self.shared, session.handle(), channel, &request, pty).await {
                Ok(started) => {
                    let chan = self.channels.get_mut(&channel).expect("channel kept");
                    chan.input = started.input;
                    chan.master = started.master;
                    chan.group = Some(started.group);
                    chan.task = Some(started.task);
                    session.channel_success(channel)
                }
                Err(error) => {
                    let handle = session.handle();
                    session.channel_failure(channel)?;
                    let _ = handle
                        .extended_data(channel, 1, Bytes::from(format!("{error}\r\n")))
                        .await;
                    let _ = handle.exit_status_request(channel, 1).await;
                    let _ = handle.eof(channel).await;
                    let _ = handle.close(channel).await;
                    Ok(())
                }
            }
        }
    }

    impl Handler for Connection {
        type Error = russh::Error;

        async fn auth_publickey_offered(
            &mut self,
            user: &str,
            offer: PublicKeyOffer<'_>,
        ) -> Result<Auth, Self::Error> {
            Ok(if self.allowed(user, offer.public_key) {
                Auth::Accept
            } else {
                Auth::reject()
            })
        }

        async fn auth_publickey(
            &mut self,
            user: &str,
            key: VerifiedPublicKey<'_>,
        ) -> Result<Auth, Self::Error> {
            if !self.allowed(user, key.public_key) {
                return Ok(Auth::reject());
            }
            // Over the limit of logged-in connections: the connection is closed.
            self.admit()?;
            Ok(Auth::Accept)
        }

        async fn channel_open_session(
            &mut self,
            channel: Channel<Msg>,
            reply: russh::server::ChannelOpenHandle,
            _session: &mut Session,
        ) -> Result<(), Self::Error> {
            if self.channels.len() + self.forwards.len() >= MAX_CHANNELS {
                return Ok(());
            }
            self.channels.insert(channel.id(), Chan::default());
            reply.accept().await;
            Ok(())
        }

        /// `ssh -L`, `ssh -D` and an editor's port forwarding, to the container's own loopback
        /// only. The connection is made from inside the container's network namespace
        /// (`silo-ctr connect`), where `127.0.0.1` is the container's; any other destination is
        /// refused. Nothing here waits for the connection: the session loop keeps running.
        async fn channel_open_direct_tcpip(
            &mut self,
            channel: Channel<Msg>,
            host_to_connect: &str,
            port_to_connect: u32,
            _originator_address: &str,
            _originator_port: u32,
            reply: ChannelOpenHandle,
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            let Some((host, port)) = loopback_destination(host_to_connect, port_to_connect) else {
                reply
                    .reject(ChannelOpenFailure::AdministrativelyProhibited)
                    .await;
                return Ok(());
            };
            if self.channels.len() + self.forwards.len() >= MAX_CHANNELS {
                reply.reject(ChannelOpenFailure::ResourceShortage).await;
                return Ok(());
            }
            let id = channel.id();
            let task = tokio::spawn(forward(
                self.shared.clone(),
                session.handle(),
                channel,
                host,
                port,
                reply,
            ));
            self.forwards.insert(id, AbortOnDrop(task));
            Ok(())
        }

        async fn signal(
            &mut self,
            channel: ChannelId,
            signal: russh::Sig,
            _session: &mut Session,
        ) -> Result<(), Self::Error> {
            // To the command's process group, as `sshd` signals the session's process.
            if let (Some(number), Some(group)) = (
                signal_number(&signal),
                self.channels
                    .get(&channel)
                    .and_then(|chan| chan.group.as_ref()),
            ) {
                group.signal(number);
            }
            Ok(())
        }

        async fn pty_request(
            &mut self,
            channel: ChannelId,
            term: &str,
            col_width: u32,
            row_height: u32,
            _pix_width: u32,
            _pix_height: u32,
            _modes: &[(russh::Pty, u32)],
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            match self.channels.get_mut(&channel) {
                Some(chan) if chan.task.is_none() => {
                    chan.pty = Some(Pty {
                        term: clean_term(term),
                        cols: col_width.clamp(1, u16::MAX.into()) as u16,
                        rows: row_height.clamp(1, u16::MAX.into()) as u16,
                    });
                    session.channel_success(channel)
                }
                _ => session.channel_failure(channel),
            }
        }

        async fn env_request(
            &mut self,
            channel: ChannelId,
            _name: &str,
            _value: &str,
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            // The client's environment is not passed on, as with an `sshd` that accepts none.
            session.channel_failure(channel)
        }

        async fn shell_request(
            &mut self,
            channel: ChannelId,
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            self.run(session, channel, Request::Shell).await
        }

        async fn exec_request(
            &mut self,
            channel: ChannelId,
            data: &[u8],
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            match std::str::from_utf8(data) {
                Ok(command) if !command.contains('\0') => {
                    self.run(session, channel, Request::Command(command.to_owned()))
                        .await
                }
                _ => session.channel_failure(channel),
            }
        }

        async fn subsystem_request(
            &mut self,
            channel: ChannelId,
            name: &str,
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            if name == "sftp" {
                // A subsystem has no terminal.
                if let Some(chan) = self.channels.get_mut(&channel) {
                    chan.pty = None;
                }
                self.run(session, channel, Request::Sftp).await
            } else {
                session.channel_failure(channel)
            }
        }

        async fn data(
            &mut self,
            channel: ChannelId,
            data: &[u8],
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            // Never waits for the command: this loop also takes the client's window updates,
            // which the command's output needs to get through.
            let Some(chan) = self.channels.get_mut(&channel) else {
                return Ok(());
            };
            if chan.input.as_ref().is_some_and(|input| !input.push(data)) {
                // The command has not read what the client sent for a long way: it is ended.
                chan.end();
                session.extended_data(
                    channel,
                    1,
                    Bytes::from_static(b"The command did not read its input.\r\n"),
                )?;
                session.exit_status_request(channel, 1)?;
                session.close(channel)?;
            }
            Ok(())
        }

        async fn channel_eof(
            &mut self,
            channel: ChannelId,
            _session: &mut Session,
        ) -> Result<(), Self::Error> {
            if let Some(chan) = self.channels.get_mut(&channel) {
                // End of input for a command; a terminal has its own end-of-file character.
                if chan.master.is_none() {
                    chan.input = None;
                }
            }
            Ok(())
        }

        async fn channel_close(
            &mut self,
            channel: ChannelId,
            _session: &mut Session,
        ) -> Result<(), Self::Error> {
            // Dropping the channel ends its command (the whole process group) or its forward.
            self.channels.remove(&channel);
            self.forwards.remove(&channel);
            Ok(())
        }

        async fn window_change_request(
            &mut self,
            channel: ChannelId,
            col_width: u32,
            row_height: u32,
            _pix_width: u32,
            _pix_height: u32,
            _session: &mut Session,
        ) -> Result<(), Self::Error> {
            if let Some(master) = self
                .channels
                .get(&channel)
                .and_then(|chan| chan.master.as_ref())
            {
                set_window_size(
                    master.as_raw_fd(),
                    col_width.clamp(1, u16::MAX.into()) as u16,
                    row_height.clamp(1, u16::MAX.into()) as u16,
                );
            }
            Ok(())
        }
    }

    fn set_window_size(fd: i32, cols: u16, rows: u16) {
        let size = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: TIOCSWINSZ reads one `winsize` from a valid pointer; the descriptor is open.
        unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, &size) };
    }

    struct Started {
        input: Option<Input>,
        master: Option<Arc<std::fs::File>>,
        group: Arc<ExecGroup>,
        task: JoinHandle<()>,
    }

    /// The `crun exec` command for a request, as the working account.
    fn plan(
        shared: &Shared,
        request: &Request,
        pty: Option<&Pty>,
        console_socket: Option<PathBuf>,
    ) -> Result<lifecycle::ExecPlan, String> {
        let user = crate::working_account::USER;
        let dir = Layout::new(&shared.paths.home).computer(&shared.name);
        let identity = lifecycle::identity(&shared.paths, &shared.tools, &shared.name, user)?;
        check_working_identity(&identity, &shared.name)?;
        let mut env = vec![
            ("USER".to_owned(), user.to_owned()),
            ("LOGNAME".to_owned(), user.to_owned()),
            ("SHELL".to_owned(), "/bin/bash".to_owned()),
        ];
        if let Some(pty) = pty {
            env.push(("TERM".to_owned(), pty.term.clone()));
        }
        // The same placeholders the engine gives its own commands.
        env.extend(crate::containers::secrets::placeholder_env(
            &lifecycle::published_secrets(&dir),
        ));
        let exec = Exec {
            name: shared.name.clone(),
            user: Some(user.to_owned()),
            env,
            workdir: Some(identity.home),
            no_start: true,
            tty: pty.is_some(),
            console_socket,
            argv: request.argv(),
            ..Exec::default()
        };
        lifecycle::exec_command(&shared.paths, &shared.tools, &shared.name, &exec, false)
    }

    async fn start(
        shared: &Arc<Shared>,
        handle: Handle,
        channel: ChannelId,
        request: &Request,
        pty: Option<Pty>,
    ) -> Result<Started, String> {
        if !lifecycle::holder_alive(&shared.paths, &shared.name) {
            return Err(format!("Start {} first.", shared.name));
        }
        match pty {
            Some(pty) => start_terminal(shared, handle, channel, request, pty).await,
            None => start_pipes(shared, handle, channel, request).await,
        }
    }

    async fn blocking<T: Send + 'static>(
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        tokio::task::spawn_blocking(work)
            .await
            .map_err(|_| "The session could not start.".to_owned())?
    }

    async fn start_pipes(
        shared: &Arc<Shared>,
        handle: Handle,
        channel: ChannelId,
        request: &Request,
    ) -> Result<Started, String> {
        let (work, request) = (shared.clone(), request.clone());
        let lifecycle::ExecPlan { command, group } =
            blocking(move || plan(&work, &request, None, None)).await?;
        let group = Arc::new(group);
        // An editor's proxy runs in a login session, not the engine's control group.
        let command = lifecycle::in_user_scope(&shared.tools, &command);
        let mut child = tokio::process::Command::from(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| format!("The session could not start: {error}"))?;
        let stdin = child.stdin.take().ok_or("The session has no input.")?;
        let stdout = child.stdout.take().ok_or("The session has no output.")?;
        let stderr = child.stderr.take().ok_or("The session has no output.")?;
        let (input, receiver) = Input::new();
        tokio::spawn(write_input(stdin, receiver, input.queued.clone()));
        let held = group.clone();
        let task = tokio::spawn(async move {
            let out = tokio::spawn(pump(stdout, handle.clone(), channel, None));
            let err = tokio::spawn(pump(stderr, handle.clone(), channel, Some(1)));
            let code = exit_code(child.wait().await);
            // What a finished command left running may still hold the pipes: do not wait for it.
            let drain = std::time::Duration::from_secs(2);
            let _ = tokio::time::timeout(drain, async {
                let _ = out.await;
                let _ = err.await;
            })
            .await;
            finish(&handle, channel, code).await;
            drop(held);
        });
        Ok(Started {
            input: Some(input),
            master: None,
            group,
            task,
        })
    }

    async fn start_terminal(
        shared: &Arc<Shared>,
        handle: Handle,
        channel: ChannelId,
        request: &Request,
        pty: Pty,
    ) -> Result<Started, String> {
        // `crun` hands the terminal's controlling side to a socket we own. The path stays short
        // (a socket path holds about 100 bytes) and private.
        let directory = tempfile::Builder::new()
            .prefix("silo-pty-")
            .tempdir()
            .map_err(|error| format!("The terminal could not start: {error}"))?;
        let socket = directory.path().join("c");
        let listener = std::os::unix::net::UnixListener::bind(&socket)
            .map_err(|error| format!("The terminal could not start: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let (work, request, term) = (shared.clone(), request.clone(), pty.clone());
        let console = socket.clone();
        let lifecycle::ExecPlan { command, group } =
            blocking(move || plan(&work, &request, Some(&term), Some(console))).await?;
        let group = Arc::new(group);
        let command = lifecycle::in_user_scope(&shared.tools, &command);
        let started = spawn_terminal(command, listener, TERMINAL_START).await;
        drop(directory);
        let (mut child, master) = match started {
            Ok(started) => started,
            Err(error) => {
                group.kill();
                return Err(error);
            }
        };
        let master = std::fs::File::from(master);
        // Reads and writes go through the blocking pool; the descriptor must block.
        // SAFETY: fcntl on an open descriptor with plain flags.
        unsafe {
            let flags = libc::fcntl(master.as_raw_fd(), libc::F_GETFL);
            if flags >= 0 {
                libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags & !libc::O_NONBLOCK);
            }
        }
        set_window_size(master.as_raw_fd(), pty.cols, pty.rows);
        let sizing = Arc::new(master.try_clone().map_err(|error| error.to_string())?);
        let reader = tokio::fs::File::from_std(
            master.try_clone().map_err(|error| error.to_string())?,
        );
        let writer = tokio::fs::File::from_std(master);
        let (input, receiver) = Input::new();
        tokio::spawn(write_input(writer, receiver, input.queued.clone()));
        let held = group.clone();
        let task = tokio::spawn(async move {
            let output = tokio::spawn(pump(reader, handle.clone(), channel, None));
            let code = exit_code(child.wait().await);
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), output).await;
            finish(&handle, channel, code).await;
            drop(held);
        });
        Ok(Started {
            input: Some(input),
            master: Some(sizing),
            group,
            task,
        })
    }

    /// How long `crun exec` has to hand over the terminal.
    const TERMINAL_START: std::time::Duration = std::time::Duration::from_secs(15);

    /// What a failed program printed on its error stream, to show with the reason it failed.
    async fn printed(stderr: Option<tokio::process::ChildStderr>) -> String {
        let Some(mut stderr) = stderr else {
            return String::new();
        };
        let mut bytes = Vec::new();
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            (&mut stderr).take(4096).read_to_end(&mut bytes),
        )
        .await;
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        lines[lines.len().saturating_sub(3)..].join(" / ")
    }

    /// Runs `command` (a `crun exec` on a terminal) and takes the terminal's controlling side from
    /// `listener`. When the command ends first, or hands nothing over in `limit`, or hands over
    /// something else, the error says why and what the command printed.
    async fn spawn_terminal(
        command: std::process::Command,
        listener: std::os::unix::net::UnixListener,
        limit: std::time::Duration,
    ) -> Result<(tokio::process::Child, OwnedFd), String> {
        let mut child = tokio::process::Command::from(command)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| format!("The terminal could not start: {error}"))?;
        let deadline = std::time::Instant::now() + limit;
        let accepted = loop {
            match listener.accept() {
                Ok((stream, _)) => break Ok(stream),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if let Ok(Some(status)) = child.try_wait() {
                        break Err(format!("The terminal command ended ({status})"));
                    }
                    if std::time::Instant::now() >= deadline {
                        break Err("The terminal did not start in time".to_owned());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                Err(error) => break Err(error.to_string()),
            }
        };
        let received = match accepted {
            Ok(stream) => {
                blocking(move || {
                    stream
                        .set_nonblocking(false)
                        .map_err(|error| error.to_string())?;
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                        .map_err(|error| error.to_string())?;
                    lifecycle::receive_descriptor(&stream)
                })
                .await
            }
            Err(error) => Err(error),
        };
        match received {
            Ok(master) => {
                // What it prints from now on is of no use, and must not fill its pipe.
                if let Some(mut stderr) = child.stderr.take() {
                    tokio::spawn(async move {
                        let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
                    });
                }
                Ok((child, master))
            }
            Err(reason) => {
                let _ = child.start_kill();
                let said = printed(child.stderr.take()).await;
                Err(if said.is_empty() {
                    format!("The terminal could not start: {reason}.")
                } else {
                    format!("The terminal could not start: {reason}: {said}")
                })
            }
        }
    }

    /// Serves a `direct-tcpip` channel: `silo-ctr connect` joins the container's network namespace
    /// and connects to its loopback, then its standard input and output are the channel.
    async fn forward(
        shared: Arc<Shared>,
        handle: Handle,
        channel: Channel<Msg>,
        host: String,
        port: u16,
        reply: ChannelOpenHandle,
    ) {
        let id = channel.id();
        match open_forward(&shared, &host, port).await {
            Ok((child, stdin, stdout)) => {
                reply.accept().await;
                let mut stream = channel.into_stream();
                let mut peer = tokio::io::join(stdout, stdin);
                let _ = tokio::io::copy_bidirectional(&mut stream, &mut peer).await;
                drop(stream);
                // `child` ends here and takes the connection with it.
                drop(child);
                let _ = handle.eof(id).await;
                let _ = handle.close(id).await;
            }
            Err(error) => {
                eprintln!("silo: ssh forward to {host}:{port} in {}: {error}", shared.name);
                reply.reject(ChannelOpenFailure::ConnectFailed).await;
            }
        }
    }

    /// Starts `silo-ctr connect` and waits until it reports the connection (one zero byte).
    async fn open_forward(
        shared: &Shared,
        host: &str,
        port: u16,
    ) -> Result<
        (
            tokio::process::Child,
            tokio::process::ChildStdin,
            tokio::process::ChildStdout,
        ),
        String,
    > {
        let state = Layout::new(&shared.paths.home).computer(&shared.name).state();
        let mut child = tokio::process::Command::new(&shared.tools.ctr)
            .arg("connect")
            .arg("--state")
            .arg(&state)
            .args(["--host", host, "--port"])
            .arg(port.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| format!("could not start: {error}"))?;
        let stdin = child.stdin.take().ok_or("no input")?;
        let mut stdout = child.stdout.take().ok_or("no output")?;
        let mut ready = [0u8; 1];
        // The helper itself gives up after 10 s.
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(12),
            stdout.read_exact(&mut ready),
        )
        .await;
        match read {
            Ok(Ok(_)) => Ok((child, stdin, stdout)),
            _ => {
                let _ = child.start_kill();
                let said = printed(child.stderr.take()).await;
                Err(if said.is_empty() {
                    "the connection was not made".to_owned()
                } else {
                    said
                })
            }
        }
    }

    fn exit_code(status: std::io::Result<std::process::ExitStatus>) -> u32 {
        use std::os::unix::process::ExitStatusExt;
        match status {
            Ok(status) => status
                .code()
                .map(|code| code as u32)
                .or_else(|| status.signal().map(|signal| 128 + signal as u32))
                .unwrap_or(1),
            Err(_) => 1,
        }
    }

    async fn finish(handle: &Handle, channel: ChannelId, code: u32) {
        let _ = handle.exit_status_request(channel, code).await;
        let _ = handle.eof(channel).await;
        let _ = handle.close(channel).await;
    }

    /// Copies the command's output to the channel until it ends. A terminal's controlling side reports
    /// the end as an I/O error, which is not a failure.
    async fn pump(
        mut source: impl AsyncRead + Unpin,
        handle: Handle,
        channel: ChannelId,
        extended: Option<u32>,
    ) {
        let mut buffer = vec![0u8; 32 * 1024];
        loop {
            let read = match source.read(&mut buffer).await {
                Ok(0) | Err(_) => return,
                Ok(read) => read,
            };
            let data = Bytes::copy_from_slice(&buffer[..read]);
            let sent = match extended {
                Some(code) => handle.extended_data(channel, code, data).await,
                None => handle.data(channel, data).await,
            };
            if sent.is_err() {
                return;
            }
        }
    }

    /// Writes what the client sends to the command until the client ends its input.
    async fn write_input(
        mut sink: impl AsyncWrite + Unpin,
        mut receiver: mpsc::UnboundedReceiver<Bytes>,
        queued: Arc<AtomicUsize>,
    ) {
        while let Some(chunk) = receiver.recv().await {
            let written = sink.write_all(&chunk).await.is_ok() && sink.flush().await.is_ok();
            queued.fetch_sub(chunk.len(), Ordering::Relaxed);
            if !written {
                return;
            }
        }
        let _ = sink.shutdown().await;
    }

    fn config(serve: &SshServe, host_key: russh::keys::PrivateKey) -> Arc<russh::server::Config> {
        Arc::new(russh::server::Config {
            auth_rejection_time: std::time::Duration::from_secs(3),
            auth_rejection_time_initial: Some(std::time::Duration::ZERO),
            methods: russh::MethodSet::from(&[russh::MethodKind::PublicKey][..]),
            max_auth_attempts: 6,
            keys: vec![host_key],
            inactivity_timeout: (!serve.no_inactivity_timeout).then_some(INACTIVITY),
            ..Default::default()
        })
    }

    fn load_host_key(path: &Path) -> Result<russh::keys::PrivateKey, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("load SSH host key: {error}"))?;
        russh::keys::decode_secret_key(&text, None)
            .map_err(|error| format!("load SSH host key: {error}"))
    }

    fn handler(shared: &Arc<Shared>, limits: Option<Limits>) -> Connection {
        Connection {
            shared: shared.clone(),
            channels: HashMap::new(),
            forwards: HashMap::new(),
            limits,
        }
    }

    /// Runs the server the command line describes until the connection (`--stdio`) or the owner
    /// ends it. Returns the exit code.
    pub fn serve(paths: &RuntimePaths, serve: &SshServe) -> Result<i32, String> {
        crate::runtime::validate_name(&serve.name).map_err(|error| error.to_string())?;
        let layout = Layout::new(&paths.home);
        let record = layout
            .computer(&serve.name)
            .read_record()
            .map_err(|_| format!("sandbox {} not found", serve.name))?;
        check_machine_id(
            record.label("silo.machine-id"),
            serve.expected_machine_id.as_deref(),
        )?;
        let tools = Tools::locate(paths).map_err(|error| error.to_string())?;
        if !lifecycle::holder_alive(paths, &serve.name) {
            return Err(format!("sandbox must already be running: start {} first.", serve.name));
        }
        let authorized = serve
            .authorized_keys
            .clone()
            .unwrap_or_else(|| default_authorized_keys(&paths.home));
        let keys = std::fs::read_to_string(&authorized)
            .map(|text| parse_authorized_keys(&text))
            .map_err(|error| {
                format!(
                    "SSH authorized keys not readable at {}: {error}",
                    authorized.display()
                )
            })?;
        if keys.is_empty() {
            return Err("SSH server has no authorized public keys".into());
        }
        let host_key = load_host_key(&host_key_path(&paths.home, &serve.name))?;
        let config = config(serve, host_key);
        let shared = Arc::new(Shared {
            paths: paths.clone(),
            tools,
            name: serve.name.clone(),
            authorized,
            connections: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
            unauthenticated: Arc::new(Semaphore::new(MAX_UNAUTHENTICATED)),
        });
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        let result = runtime.block_on(async {
            if serve.stdio {
                over_stdio(&shared, config).await
            } else {
                listen(serve, &shared, config).await
            }
        });
        // The standard input thread may still be blocked in a read.
        runtime.shutdown_timeout(std::time::Duration::from_secs(2));
        result.map(|()| 0)
    }

    async fn over_stdio(shared: &Arc<Shared>, config: Arc<russh::server::Config>) -> Result<(), String> {
        let stream = tokio::io::join(tokio::io::stdin(), tokio::io::stdout());
        let session = russh::server::run_stream(config, stream, handler(shared, None))
            .await
            .map_err(|error| error.to_string())?;
        session.await.map_err(|error| error.to_string())
    }

    async fn listen(
        serve: &SshServe,
        shared: &Arc<Shared>,
        config: Arc<russh::server::Config>,
    ) -> Result<(), String> {
        let host = serve.host.as_deref().unwrap_or("127.0.0.1");
        let port = serve.port.unwrap_or(2222);
        let listener = tokio::net::TcpListener::bind((host, port))
            .await
            .map_err(|error| format!("SSH could not listen on {host}:{port}: {error}"))?;
        let report = serve.exit_on_stdin_close;
        if report {
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{READY_LINE}");
            let _ = out.flush();
        }
        let owner_gone = owner_stdin_closed(serve.exit_on_stdin_close);
        tokio::pin!(owner_gone);
        let mut failures = 0u32;
        loop {
            tokio::select! {
                biased;
                _ = &mut owner_gone => return Ok(()),
                accepted = listener.accept() => {
                    let stream = match accepted {
                        Ok((stream, _)) => {
                            failures = 0;
                            stream
                        }
                        Err(error) => {
                            // Out of descriptors, say: the listener stays up and tries again.
                            failures = failures.saturating_add(1);
                            eprintln!("silo: SSH accept failed: {error}");
                            tokio::select! {
                                biased;
                                _ = &mut owner_gone => return Ok(()),
                                _ = tokio::time::sleep(accept_backoff(failures)) => {}
                            }
                            continue;
                        }
                    };
                    // Too many connections still logging in: this one is closed unanswered.
                    let Ok(pending) = shared.unauthenticated.clone().try_acquire_owned() else {
                        continue;
                    };
                    tokio::spawn(serve_connection(
                        stream,
                        config.clone(),
                        shared.clone(),
                        pending,
                        LOGIN_GRACE,
                        report,
                    ));
                }
            }
        }
    }

    /// One connection of the listener, from `accept` to the end. It has `grace` to log in, however
    /// long the inactivity timeout is (or if there is none); the key exchange counts in that time.
    async fn serve_connection<S>(
        stream: S,
        config: Arc<russh::server::Config>,
        shared: Arc<Shared>,
        pending: tokio::sync::OwnedSemaphorePermit,
        grace: std::time::Duration,
        report: bool,
    ) where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        line(report, SESSION_OPEN_LINE);
        let deadline = tokio::time::Instant::now() + grace;
        let (authenticated, mut login) = watch::channel(false);
        let limits = Limits {
            pending: Some(pending),
            active: None,
            authenticated,
        };
        let started = tokio::time::timeout_at(
            deadline,
            russh::server::run_stream(config, stream, handler(&shared, Some(limits))),
        )
        .await;
        if let Ok(Ok(mut session)) = started {
            let handle = session.handle();
            let logged_in = async {
                if tokio::time::timeout_at(deadline, login.wait_for(|ok| *ok))
                    .await
                    .is_ok()
                {
                    std::future::pending::<()>().await;
                }
            };
            tokio::select! {
                _ = &mut session => {}
                _ = logged_in => {
                    let _ = handle
                        .disconnect(
                            russh::Disconnect::ByApplication,
                            "login time expired".into(),
                            String::new(),
                        )
                        .await;
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), session).await;
                }
            }
        }
        line(report, SESSION_CLOSED_LINE);
    }

    fn line(report: bool, text: &str) {
        if report {
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{text}");
            let _ = out.flush();
        }
    }

    /// Completes when the owner closes standard input (only when asked to watch it).
    async fn owner_stdin_closed(watch: bool) {
        if !watch {
            std::future::pending::<()>().await;
        }
        let (sender, receiver) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 128];
            let mut input = std::io::stdin().lock();
            loop {
                match std::io::Read::read(&mut input, &mut buffer) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            let _ = sender.send(());
        });
        let _ = receiver.await;
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use russh::keys::{Algorithm, PrivateKey, PrivateKeyWithHashAlg};

        struct Client;

        impl russh::client::Handler for Client {
            type Error = russh::Error;

            async fn check_server_key(
                &mut self,
                _: &russh::keys::PublicKeyOrCertificate,
            ) -> Result<bool, Self::Error> {
                Ok(true)
            }
        }

        fn key() -> PrivateKey {
            PrivateKey::random(&mut russh::keys::key::safe_rng(), Algorithm::Ed25519).unwrap()
        }

        fn authorized_line(key: &PrivateKey, comment: &str) -> String {
            let line = key.public_key().to_openssh().unwrap();
            format!("{line} {comment}\n")
        }

        struct Fixture {
            directory: tempfile::TempDir,
            shared: Arc<Shared>,
            config: Arc<russh::server::Config>,
        }

        fn fixture(authorized: &str) -> Fixture {
            let directory = tempfile::tempdir().unwrap();
            let file = directory.path().join("authorized_keys");
            std::fs::write(&file, authorized).unwrap();
            let paths = crate::test_support::paths(directory.path());
            let none = || PathBuf::new();
            let shared = Arc::new(Shared {
                paths,
                tools: Tools {
                    ctr: none(),
                    init: none(),
                    gateway: none(),
                    crun: none(),
                    skopeo: none(),
                    umoci: none(),
                    systemd_run: none(),
                    systemctl: none(),
                },
                name: "box".into(),
                authorized: file,
                connections: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
                unauthenticated: Arc::new(Semaphore::new(MAX_UNAUTHENTICATED)),
            });
            let config = config(&SshServe::default(), key());
            Fixture {
                directory,
                shared,
                config,
            }
        }

        async fn connect(
            fixture: &Fixture,
        ) -> russh::client::Handle<Client> {
            let (client_end, server_end) = tokio::io::duplex(256 * 1024);
            // The server waits for the client's identification before it answers.
            let (config, shared) = (fixture.config.clone(), fixture.shared.clone());
            tokio::spawn(async move {
                let session = russh::server::run_stream(config, server_end, handler(&shared, None))
                    .await
                    .unwrap();
                let _ = session.await;
            });
            russh::client::connect_stream(Arc::new(russh::client::Config::default()), client_end, Client)
                .await
                .unwrap()
        }

        /// A fixture whose tools run `ctr` as `silo-ctr`.
        fn fixture_with_ctr(authorized: &str, script: &str) -> Fixture {
            use std::os::unix::fs::PermissionsExt;
            let mut fixture = fixture(authorized);
            let ctr = fixture.directory.path().join("silo-ctr");
            std::fs::write(&ctr, script).unwrap();
            std::fs::set_permissions(&ctr, std::fs::Permissions::from_mode(0o755)).unwrap();
            let mut shared = (*fixture.shared).clone();
            shared.tools.ctr = ctr;
            fixture.shared = Arc::new(shared);
            fixture
        }

        /// A client over `serve_connection`, the way the listener serves an accepted socket.
        fn listener_connection(
            fixture: &Fixture,
            grace: std::time::Duration,
        ) -> (tokio::io::DuplexStream, tokio::task::JoinHandle<()>) {
            let (client_end, server_end) = tokio::io::duplex(256 * 1024);
            let pending = fixture
                .shared
                .unauthenticated
                .clone()
                .try_acquire_owned()
                .unwrap();
            let task = tokio::spawn(serve_connection(
                server_end,
                fixture.config.clone(),
                fixture.shared.clone(),
                pending,
                grace,
                false,
            ));
            (client_end, task)
        }

        async fn client_over(stream: tokio::io::DuplexStream) -> russh::client::Handle<Client> {
            russh::client::connect_stream(Arc::new(russh::client::Config::default()), stream, Client)
                .await
                .unwrap()
        }

        async fn login(client: &mut russh::client::Handle<Client>, key: &PrivateKey) -> bool {
            client
                .authenticate_publickey("silo", PrivateKeyWithHashAlg::new(Arc::new(key.clone()), None))
                .await
                .is_ok_and(|result| result.success())
        }

        async fn authenticates(fixture: &Fixture, user: &str, key: &PrivateKey) -> bool {
            let mut client = connect(fixture).await;
            client
                .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key.clone()), None))
                .await
                .unwrap()
                .success()
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn only_a_listed_key_opens_a_session_for_the_working_account() {
            let (allowed, other) = (key(), key());
            let fixture = fixture(&authorized_line(&allowed, "me"));
            assert!(authenticates(&fixture, "silo", &allowed).await);
            assert!(!authenticates(&fixture, "silo", &other).await);
            // The working account is the only login.
            assert!(!authenticates(&fixture, "root", &allowed).await);
            // The file is read at every attempt: removing the key ends its access at once.
            std::fs::write(&fixture.shared.authorized, authorized_line(&other, "you")).unwrap();
            assert!(!authenticates(&fixture, "silo", &allowed).await);
            assert!(authenticates(&fixture, "silo", &other).await);
            drop(fixture.directory);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn a_session_on_a_computer_that_is_not_running_says_so_and_exits_with_an_error() {
            let allowed = key();
            let fixture = fixture(&authorized_line(&allowed, "me"));
            let mut client = connect(&fixture).await;
            assert!(client
                .authenticate_publickey("silo", PrivateKeyWithHashAlg::new(Arc::new(allowed), None))
                .await
                .unwrap()
                .success());
            let mut channel = client.channel_open_session().await.unwrap();
            // The request is refused, and the reason arrives on the error stream.
            let _ = channel.exec(true, "id").await;
            let mut error = String::new();
            let mut status = None;
            while let Some(message) = channel.wait().await {
                match message {
                    russh::ChannelMsg::ExtendedData { data, .. } => {
                        error.push_str(&String::from_utf8_lossy(&data))
                    }
                    russh::ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
                    _ => {}
                }
                if status.is_some() {
                    break;
                }
            }
            assert!(error.contains("Start box first."), "{error:?}");
            assert_eq!(status, Some(1));
        }

        #[test]
        fn a_command_that_does_not_read_cannot_hold_up_the_session_or_grow_without_limit() {
            let (input, _receiver) = Input::new();
            let chunk = vec![7u8; 1024 * 1024];
            // Nothing reads: every push still returns at once, up to the limit.
            for _ in 0..INPUT_QUEUE_LIMIT / chunk.len() {
                assert!(input.push(&chunk));
            }
            assert!(!input.push(&[0]), "the queue grew past its limit");
            assert_eq!(input.queued.load(Ordering::Relaxed), INPUT_QUEUE_LIMIT);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn the_input_queue_empties_into_the_command_while_the_session_keeps_going() {
            let (input, receiver) = Input::new();
            // A sink that takes a few kilobytes and then waits for its reader.
            let (mut reader, writer) = tokio::io::duplex(4096);
            let writer_task = tokio::spawn(write_input(writer, receiver, input.queued.clone()));
            let chunk = vec![1u8; 64 * 1024];
            let started = std::time::Instant::now();
            for _ in 0..64 {
                assert!(input.push(&chunk));
            }
            // 4 MiB were handed over without anything reading them.
            assert!(started.elapsed() < std::time::Duration::from_secs(5));
            let queued = input.queued.clone();
            drop(input);
            let mut received = 0usize;
            let mut buffer = vec![0u8; 8192];
            loop {
                let count = reader.read(&mut buffer).await.unwrap();
                if count == 0 {
                    break;
                }
                received += count;
            }
            writer_task.await.unwrap();
            assert_eq!(received, 64 * chunk.len());
            assert_eq!(queued.load(Ordering::Relaxed), 0);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn a_connection_that_does_not_log_in_within_the_grace_is_closed() {
            let fixture = fixture("");
            // Says nothing at all: not even its version.
            let (mut silent, task) =
                listener_connection(&fixture, std::time::Duration::from_millis(300));
            let mut seen = Vec::new();
            tokio::time::timeout(std::time::Duration::from_secs(10), silent.read_to_end(&mut seen))
                .await
                .expect("the server kept a silent connection open")
                .unwrap();
            task.await.unwrap();
            // Its place was given back.
            assert_eq!(fixture.shared.unauthenticated.available_permits(), MAX_UNAUTHENTICATED);
            assert_eq!(fixture.shared.connections.available_permits(), MAX_CONNECTIONS);

            // One that connects and starts the key exchange but never proves a key.
            let (stream, task) =
                listener_connection(&fixture, std::time::Duration::from_millis(500));
            let client = client_over(stream).await;
            tokio::time::timeout(std::time::Duration::from_secs(10), task)
                .await
                .expect("the server kept a connection that never logged in")
                .unwrap();
            drop(client);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn a_connection_that_logged_in_outlives_the_grace_and_is_the_only_kind_that_counts() {
            let allowed = key();
            let fixture = fixture(&authorized_line(&allowed, "me"));
            let (stream, task) =
                listener_connection(&fixture, std::time::Duration::from_millis(500));
            let mut client = client_over(stream).await;
            assert_eq!(fixture.shared.unauthenticated.available_permits(), MAX_UNAUTHENTICATED - 1);
            assert!(login(&mut client, &allowed).await);
            // Logged in: it left the unauthenticated ones and now counts against the cap.
            assert_eq!(fixture.shared.unauthenticated.available_permits(), MAX_UNAUTHENTICATED);
            assert_eq!(fixture.shared.connections.available_permits(), MAX_CONNECTIONS - 1);
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
            assert!(!task.is_finished(), "the grace ended a connection that had logged in");
            client.channel_open_session().await.unwrap();
            drop(client);
            tokio::time::timeout(std::time::Duration::from_secs(10), task)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(fixture.shared.connections.available_permits(), MAX_CONNECTIONS);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn idle_sockets_cannot_lock_out_a_login_and_the_cap_closes_the_extra_logged_in_one() {
            let allowed = key();
            let mut fixture = fixture(&authorized_line(&allowed, "me"));
            let mut shared = (*fixture.shared).clone();
            shared.connections = Arc::new(Semaphore::new(1));
            fixture.shared = Arc::new(shared);
            // More idle sockets than the cap of logged-in connections.
            let idle: Vec<_> = (0..4)
                .map(|_| listener_connection(&fixture, std::time::Duration::from_secs(60)))
                .collect();
            let (stream, _first) =
                listener_connection(&fixture, std::time::Duration::from_secs(60));
            let mut client = client_over(stream).await;
            assert!(login(&mut client, &allowed).await, "idle sockets locked out a login");
            // The cap is on logged-in connections: the next one to log in is closed.
            let (stream, _second) =
                listener_connection(&fixture, std::time::Duration::from_secs(60));
            let mut extra = client_over(stream).await;
            assert!(!login(&mut extra, &allowed).await, "a connection over the cap logged in");
            drop(idle);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn a_forward_reaches_the_loopback_of_the_container_and_nothing_else() {
            let allowed = key();
            // Stands in for `silo-ctr connect`: reports the connection, then echoes.
            let fixture = fixture_with_ctr(
                &authorized_line(&allowed, "me"),
                "#!/bin/sh\nprintf '\\000'\nexec cat\n",
            );
            let mut client = connect(&fixture).await;
            assert!(login(&mut client, &allowed).await);
            let mut channel = client
                .channel_open_direct_tcpip("127.0.0.1", 8080, "127.0.0.1", 40000)
                .await
                .expect("a forward to the loopback was refused");
            channel.data(&b"through the forward"[..]).await.unwrap();
            let mut echoed = Vec::new();
            while echoed.len() < b"through the forward".len() {
                match tokio::time::timeout(std::time::Duration::from_secs(10), channel.wait())
                    .await
                    .expect("the forward did not answer")
                {
                    Some(russh::ChannelMsg::Data { data }) => echoed.extend_from_slice(&data),
                    Some(_) => {}
                    None => break,
                }
            }
            assert_eq!(echoed, b"through the forward");
            channel.eof().await.unwrap();
            for (host, port) in [
                ("10.0.0.5", 80),
                ("example.com", 80),
                ("0.0.0.0", 80),
                ("169.254.169.254", 80),
                ("localhost", 0),
            ] {
                assert!(
                    client
                        .channel_open_direct_tcpip(host, port, "127.0.0.1", 40001)
                        .await
                        .is_err(),
                    "a forward to {host}:{port} was accepted"
                );
            }
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn a_forward_whose_connection_fails_is_refused() {
            let allowed = key();
            let fixture = fixture_with_ctr(
                &authorized_line(&allowed, "me"),
                "#!/bin/sh\necho 'connect to 127.0.0.1:9: Connection refused' >&2\nexit 1\n",
            );
            let mut client = connect(&fixture).await;
            assert!(login(&mut client, &allowed).await);
            assert!(client
                .channel_open_direct_tcpip("127.0.0.1", 9, "127.0.0.1", 40000)
                .await
                .is_err());
            // The session is still usable.
            client.channel_open_session().await.unwrap();
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn a_terminal_command_that_ends_before_the_terminal_exists_says_why_at_once() {
            let directory = tempfile::tempdir().unwrap();
            let listener =
                std::os::unix::net::UnixListener::bind(directory.path().join("c")).unwrap();
            listener.set_nonblocking(true).unwrap();
            let mut command = std::process::Command::new("sh");
            command.args(["-c", "echo 'crun: the container is not running' >&2; exit 3"]);
            let started = std::time::Instant::now();
            let error = spawn_terminal(command, listener, std::time::Duration::from_secs(60))
                .await
                .unwrap_err();
            assert!(
                error.contains("crun: the container is not running") && error.contains("exit"),
                "{error}"
            );
            assert!(started.elapsed() < std::time::Duration::from_secs(20), "waited out the limit");
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn a_terminal_command_that_hands_nothing_over_is_ended_with_the_reason() {
            let directory = tempfile::tempdir().unwrap();
            let listener =
                std::os::unix::net::UnixListener::bind(directory.path().join("c")).unwrap();
            listener.set_nonblocking(true).unwrap();
            let mut command = std::process::Command::new("sh");
            command.args(["-c", "echo 'waiting for a terminal' >&2; sleep 60"]);
            let error = spawn_terminal(command, listener, std::time::Duration::from_millis(400))
                .await
                .unwrap_err();
            assert!(
                error.contains("did not start in time") && error.contains("waiting for a terminal"),
                "{error}"
            );
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn an_unknown_subsystem_and_an_environment_are_refused() {
            let allowed = key();
            let fixture = fixture(&authorized_line(&allowed, "me"));
            let mut client = connect(&fixture).await;
            assert!(client
                .authenticate_publickey("silo", PrivateKeyWithHashAlg::new(Arc::new(allowed), None))
                .await
                .unwrap()
                .success());
            let mut channel = client.channel_open_session().await.unwrap();
            channel.set_env(true, "LANG", "C").await.unwrap();
            let refused = loop {
                match channel.wait().await {
                    Some(russh::ChannelMsg::Failure) => break true,
                    Some(russh::ChannelMsg::Success) | None => break false,
                    Some(_) => {}
                }
            };
            assert!(refused, "an environment variable was accepted");
            channel.request_subsystem(true, "netconf").await.unwrap();
            let refused = loop {
                match channel.wait().await {
                    Some(russh::ChannelMsg::Failure) => break true,
                    Some(russh::ChannelMsg::Success) | None => break false,
                    Some(_) => {}
                }
            };
            assert!(refused, "an unknown subsystem was accepted");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::{Algorithm, PrivateKey};

    fn line(key: &PrivateKey, comment: &str) -> String {
        format!("{} {comment}", key.public_key().to_openssh().unwrap())
    }

    fn key() -> PrivateKey {
        PrivateKey::random(&mut russh::keys::key::safe_rng(), Algorithm::Ed25519).unwrap()
    }

    #[test]
    fn the_listener_prints_the_line_the_callers_wait_for() {
        // `ssh_access::follow_listener` waits for exactly this line.
        assert_eq!(READY_LINE, "SILO_SSH_READY");
    }

    #[test]
    fn authorized_keys_are_plain_key_lines_without_options() {
        let (a, b, c) = (key(), key(), key());
        let text = format!(
            "# comment\n\n{}\nrestrict,command=\"echo\" {}\n  {}  \nnot a key\nssh-ed25519 AAAA\n",
            line(&a, "silo-controller:1"),
            line(&b, "forced"),
            line(&c, ""),
        );
        let keys = parse_authorized_keys(&text);
        assert_eq!(keys.len(), 2);
        assert!(is_authorized(&keys, a.public_key()));
        assert!(is_authorized(&keys, c.public_key()));
        // A key listed with options is not accepted: the options would be dropped.
        assert!(!is_authorized(&keys, b.public_key()));
        assert!(!is_authorized(&[], a.public_key()));
    }

    #[test]
    fn a_key_matches_whatever_its_comment_says() {
        let a = key();
        let keys = parse_authorized_keys(&line(&a, "old comment"));
        let mut offered = a.public_key().clone();
        offered.set_comment("another");
        assert!(is_authorized(&keys, &offered));
    }

    #[test]
    fn a_changed_machine_identity_is_refused() {
        assert!(check_machine_id(Some("abc"), Some("abc")).is_ok());
        assert!(check_machine_id(Some("abc"), None).is_ok());
        assert!(check_machine_id(None, None).is_ok());
        for (actual, expected) in [(Some("abd"), "abc"), (None, "abc")] {
            assert_eq!(
                check_machine_id(actual, Some(expected)).unwrap_err(),
                "sandbox identity changed; SSH access refused"
            );
        }
    }

    #[test]
    fn requests_map_to_the_commands_run_as_the_working_account() {
        assert_eq!(Request::Shell.argv(), ["/bin/bash", "-l"]);
        assert_eq!(
            Request::Command("echo 'a b'".into()).argv(),
            ["/bin/bash", "-c", "echo 'a b'"]
        );
        assert_eq!(Request::Sftp.argv(), [SFTP_SERVER]);
        assert_eq!(SFTP_SERVER, "/usr/lib/openssh/sftp-server");
    }

    #[test]
    fn a_terminal_type_is_a_name_or_xterm() {
        assert_eq!(clean_term("xterm-256color"), "xterm-256color");
        assert_eq!(clean_term("tmux-256color"), "tmux-256color");
        for bad in ["", "a b", "x;y", "$(id)", &"a".repeat(65)] {
            assert_eq!(clean_term(bad), "xterm", "{bad:?}");
        }
    }

    #[test]
    fn the_files_the_callers_prepare_are_where_the_server_looks() {
        let home = Path::new("/h");
        // The editor writes its shared file and the computer's host key here.
        assert_eq!(default_authorized_keys(home), Path::new("/h/ssh/authorized_keys"));
        assert_eq!(
            host_key_path(home, "dev"),
            Path::new("/h/sandboxes/dev/ssh/host_ed25519")
        );
    }

    #[test]
    fn a_forward_may_only_reach_the_containers_loopback() {
        let ok = |host: &str, port: u32| loopback_destination(host, port);
        assert_eq!(ok("localhost", 8080), Some(("localhost".into(), 8080)));
        assert_eq!(ok("LocalHost", 1), Some(("LocalHost".into(), 1)));
        assert_eq!(ok("127.0.0.1", 65535), Some(("127.0.0.1".into(), 65535)));
        assert_eq!(ok("127.8.9.10", 22), Some(("127.8.9.10".into(), 22)));
        assert_eq!(ok("::1", 22), Some(("::1".into(), 22)));
        assert_eq!(ok("[::1]", 22), Some(("::1".into(), 22)));
        for host in [
            "",
            "example.com",
            "localhost.example.com",
            "10.0.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "::",
            "::ffff:10.0.0.1",
            "[::ffff:127.0.0.1",
            "127.0.0.1 ",
            "127.0.0.1:80",
        ] {
            assert_eq!(ok(host, 80), None, "{host:?}");
        }
        // The port is 1..=65535.
        assert_eq!(ok("localhost", 0), None);
        assert_eq!(ok("localhost", 65536), None);
        assert_eq!(ok("localhost", u32::MAX), None);
    }

    #[test]
    fn the_signals_a_client_sends_map_to_the_systems() {
        use russh::Sig;
        assert_eq!(signal_number(&Sig::INT), Some(libc::SIGINT));
        assert_eq!(signal_number(&Sig::TERM), Some(libc::SIGTERM));
        assert_eq!(signal_number(&Sig::HUP), Some(libc::SIGHUP));
        assert_eq!(signal_number(&Sig::KILL), Some(libc::SIGKILL));
        assert_eq!(signal_number(&Sig::USR1), Some(libc::SIGUSR1));
        assert_eq!(signal_number(&Sig::Custom("USR2@example.com".into())), None);
    }

    #[test]
    fn the_account_must_be_the_one_the_id_mapping_gives_the_engines_user() {
        let identity = |uid| super::super::users::Identity {
            uid,
            gid: uid,
            home: "/home/silo".into(),
            groups: vec![],
        };
        assert!(check_working_identity(&identity(1001), "box").is_ok());
        for uid in [0, 1000, 1002, 65534] {
            let error = check_working_identity(&identity(uid), "box").unwrap_err();
            assert!(
                error.contains("silo") && error.contains("box") && error.contains(&uid.to_string()),
                "{error}"
            );
        }
    }

    #[test]
    fn the_listener_backs_off_after_a_failed_accept_and_never_for_long() {
        let waits: Vec<u64> = (1..=8)
            .map(|failures| accept_backoff(failures).as_millis() as u64)
            .collect();
        assert_eq!(waits, [50, 100, 200, 400, 800, 1600, 2000, 2000]);
        assert_eq!(accept_backoff(0).as_millis(), 50);
        assert_eq!(accept_backoff(u32::MAX).as_millis(), 2000);
    }
}
