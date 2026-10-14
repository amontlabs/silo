//! The screen of a macOS computer for a device that does not run it: the guest's own Screen
//! Sharing (VNC) server on its NAT address, signed in to with the account setup created. The
//! computer-use setup step turns the service on; nothing is installed for this. This device
//! hands out the credentials and a byte stream to the port, and resizes the framework display.
use super::MacosContext;
use super::{
    computer, engine, guest_access, layout_and_record,
    store::{SetupProgress, State},
};
use serde::Serialize;
use std::{
    net::{SocketAddr, TcpStream},
    thread,
    time::{Duration, Instant},
};

/// Screen Sharing's port inside the guest.
const PORT: u16 = 5900;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RFB_GREETING_TIMEOUT: Duration = Duration::from_secs(5);
const RFB_GREETING_POLL: Duration = Duration::from_millis(100);
const RFB_GREETING_BYTES: usize = 12;
pub(super) const MIN_WIDTH: u32 = 640;
pub(super) const MIN_HEIGHT: u32 = 400;
pub(super) const MAX_WIDTH: u32 = 7680;
pub(super) const MAX_HEIGHT: u32 = 4320;
/// The framebuffer size of a computer's display before anything resizes it.
const DEFAULT_SIZE: (u32, u32) = (2560, 1600);

pub(super) const SHARING_UNAVAILABLE: &str = "Screen Sharing is not available in this computer yet. Silo turns it on when it sets up computer use; a computer set up earlier is updated in the background shortly after it starts. Try again in a minute.";
const RFB_UNAVAILABLE: &str = "Screen Sharing accepted TCP but did not return a valid RFB greeting. Try again once the computer finishes starting.";

/// What a viewer needs to sign in: the guest account and the framebuffer size at this moment.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplaySession {
    pub username: String,
    pub password: String,
    pub width: u32,
    pub height: u32,
}

/// A requested display size inside what the framework and a viewer can use, with even sides.
pub(crate) fn clamp_size(width: u64, height: u64) -> (u32, u32) {
    let fit =
        |value: u64, min: u32, max: u32| (value.clamp(u64::from(min), u64::from(max)) as u32) & !1;
    (
        fit(width, MIN_WIDTH, MAX_WIDTH),
        fit(height, MIN_HEIGHT, MAX_HEIGHT),
    )
}

/// Why the screen of a computer in this state cannot be shown, if so.
fn ready(state: State, installed: bool, setup: SetupProgress) -> Result<(), String> {
    match state {
        State::Running => {}
        State::Stopped | State::Failed => {
            return Err("The computer is not running. Start it to show its screen.".into())
        }
        State::Starting => return Err("The computer is still starting.".into()),
        State::Stopping => return Err("The computer is stopping.".into()),
        State::Preparing
        | State::Copying
        | State::Downloading
        | State::Installing
        | State::SettingUp => return Err(
            "The computer is still being set up. Its screen is available once setup has finished."
                .into(),
        ),
    }
    if !installed || !setup.computer_use || setup.needs_personalizing {
        return Err(
            "Setup has not finished. The screen is available once the computer has been set up for computer use."
                .into(),
        );
    }
    Ok(())
}

fn checked(
    app: &MacosContext,
    id: &str,
) -> Result<(super::store::Layout, super::store::Record), String> {
    let (record, state) = computer(id)?;
    ready(state, record.installed, record.setup)?;
    layout_and_record(app, id)
}

fn address(record: &super::store::Record) -> Result<SocketAddr, String> {
    guest_access::endpoint(record, PORT)
}

/// Confirms the remote service speaks RFB before exposing a display stream. `peek` leaves the
/// server greeting untouched for the viewer that takes over this connection.
fn verify_rfb_greeting(stream: &mut TcpStream, timeout: Duration) -> Result<(), String> {
    stream
        .set_read_timeout(Some(RFB_GREETING_POLL))
        .map_err(|_| RFB_UNAVAILABLE.to_string())?;
    let deadline = Instant::now() + timeout;
    loop {
        let mut greeting = [0u8; RFB_GREETING_BYTES];
        match stream.peek(&mut greeting) {
            Ok(RFB_GREETING_BYTES)
                if greeting.starts_with(b"RFB 003.")
                    && greeting[8..11].iter().all(u8::is_ascii_digit)
                    && greeting[11] == b'\n' =>
            {
                stream
                    .set_read_timeout(None)
                    .map_err(|_| RFB_UNAVAILABLE.to_string())?;
                return Ok(());
            }
            Ok(0) => return Err(RFB_UNAVAILABLE.to_string()),
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return Err(RFB_UNAVAILABLE.to_string()),
        }
        if Instant::now() >= deadline {
            return Err(RFB_UNAVAILABLE.to_string());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

/// Opens a connection to the guest's Screen Sharing port, for the owner to relay.
pub(crate) fn connect(app: &MacosContext, id: &str) -> Result<TcpStream, String> {
    let (_, record) = checked(app, id)?;
    let target = address(&record)?;
    let mut stream = TcpStream::connect_timeout(&target, CONNECT_TIMEOUT)
        .map_err(|_| SHARING_UNAVAILABLE.to_string())?;
    verify_rfb_greeting(&mut stream, RFB_GREETING_TIMEOUT)?;
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

/// The sign-in for a running, set-up computer whose Screen Sharing answers.
pub(crate) fn session(app: &MacosContext, id: &str) -> Result<DisplaySession, String> {
    let (layout, record) = checked(app, id)?;
    let target = address(&record)?;
    let mut stream = TcpStream::connect_timeout(&target, CONNECT_TIMEOUT)
        .map_err(|_| SHARING_UNAVAILABLE.to_string())?;
    verify_rfb_greeting(&mut stream, RFB_GREETING_TIMEOUT)?;
    let account = guest_access::account(&layout)?;
    let (width, height) = engine::display_size(app, id).unwrap_or(DEFAULT_SIZE);
    Ok(DisplaySession {
        username: account.user,
        password: account.password,
        width,
        height,
    })
}

/// Resizes the computer's display to the pixel size of a viewer's window. The size is clamped
/// and the size the framework applied is returned.
pub(crate) fn resize(
    app: &MacosContext,
    id: &str,
    width: u64,
    height: u64,
) -> Result<(u32, u32), String> {
    checked(app, id)?;
    let (width, height) = clamp_size(width, height);
    engine::reconfigure_display(app, id, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn done() -> SetupProgress {
        SetupProgress {
            account: true,
            sip: true,
            developer_tools: true,
            computer_use: true,
            clipboard: true,
            roots: true,
            needs_personalizing: false,
            proxy_bootstrap: true,
        }
    }

    #[test]
    fn sizes_are_clamped_and_even() {
        assert_eq!(clamp_size(1, 1), (MIN_WIDTH, MIN_HEIGHT));
        assert_eq!(clamp_size(u64::MAX, u64::MAX), (MAX_WIDTH, MAX_HEIGHT));
        assert_eq!(clamp_size(1921, 1081), (1920, 1080));
        assert_eq!(clamp_size(2560, 1600), (2560, 1600));
        assert_eq!(clamp_size(641, 401), (640, 400));
    }

    #[test]
    fn only_a_running_set_up_computer_shows_its_screen() {
        assert!(ready(State::Running, true, done()).is_ok());
        for state in [State::Stopped, State::Failed] {
            assert!(ready(state, true, done())
                .unwrap_err()
                .contains("not running"));
        }
        assert!(ready(State::Starting, true, done())
            .unwrap_err()
            .contains("starting"));
        assert!(ready(State::Stopping, true, done())
            .unwrap_err()
            .contains("stopping"));
        for state in [
            State::Preparing,
            State::Copying,
            State::Downloading,
            State::Installing,
            State::SettingUp,
        ] {
            assert!(ready(state, true, done())
                .unwrap_err()
                .contains("being set up"));
        }
    }

    #[test]
    fn a_running_computer_without_finished_computer_use_setup_has_no_screen() {
        let mut setup = done();
        setup.computer_use = false;
        assert!(ready(State::Running, true, setup)
            .unwrap_err()
            .contains("Setup has not finished"));
        let mut copy = done();
        copy.needs_personalizing = true;
        assert!(ready(State::Running, true, copy).is_err());
        assert!(ready(State::Running, false, done()).is_err());
    }

    #[test]
    fn display_readiness_checks_an_rfb_greeting_without_consuming_it() {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(b"RFB 003.889\n").unwrap();
        });
        let mut stream = TcpStream::connect(address).unwrap();
        verify_rfb_greeting(&mut stream, Duration::from_secs(1)).unwrap();
        let mut greeting = [0u8; RFB_GREETING_BYTES];
        stream.read_exact(&mut greeting).unwrap();
        assert_eq!(&greeting, b"RFB 003.889\n");
        server.join().unwrap();
    }

    #[test]
    fn display_readiness_rejects_a_non_rfb_service_with_a_stable_error() {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.write_all(b"HTTP/1.1 200").unwrap();
        });
        let mut stream = TcpStream::connect(address).unwrap();
        assert_eq!(
            verify_rfb_greeting(&mut stream, Duration::from_secs(1)).unwrap_err(),
            RFB_UNAVAILABLE
        );
        server.join().unwrap();
    }

    #[test]
    fn display_readiness_times_out_when_tcp_opens_without_a_greeting() {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(150));
        });
        let mut stream = TcpStream::connect(address).unwrap();
        let started = Instant::now();
        assert_eq!(
            verify_rfb_greeting(&mut stream, Duration::from_millis(30)).unwrap_err(),
            RFB_UNAVAILABLE
        );
        assert!(started.elapsed() < Duration::from_millis(500));
        server.join().unwrap();
    }
}
