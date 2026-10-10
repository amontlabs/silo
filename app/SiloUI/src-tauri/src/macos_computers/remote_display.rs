//! The screen of a macOS computer for a device that does not run it: the guest's own Screen
//! Sharing (VNC) server on its NAT address, signed in to with the account setup created. The
//! computer-use setup step turns the service on; nothing is installed for this. This device
//! hands out the credentials and a byte stream to the port, and resizes the framework display.
use super::{
    computer, engine, guest_access, layout_and_record,
    store::{SetupProgress, State},
};
use serde::Serialize;
use std::{
    net::{SocketAddr, TcpStream},
    time::Duration,
};
use tauri::AppHandle;

/// Screen Sharing's port inside the guest.
const PORT: u16 = 5900;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const MIN_WIDTH: u32 = 640;
pub(super) const MIN_HEIGHT: u32 = 400;
pub(super) const MAX_WIDTH: u32 = 7680;
pub(super) const MAX_HEIGHT: u32 = 4320;
/// The framebuffer size of a computer's display before anything resizes it.
const DEFAULT_SIZE: (u32, u32) = (2560, 1600);

const SHARING_UNAVAILABLE: &str = "Screen Sharing is not available in this computer yet. Silo turns it on when it sets up computer use; a computer set up earlier is updated in the background shortly after it starts. Try again in a minute.";

/// What a viewer needs to sign in: the guest account and the framebuffer size at this moment.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DisplaySession {
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
    app: &AppHandle,
    id: &str,
) -> Result<(super::store::Layout, super::store::Record), String> {
    let (record, state) = computer(id)?;
    ready(state, record.installed, record.setup)?;
    layout_and_record(app, id)
}

fn address(record: &super::store::Record) -> Result<SocketAddr, String> {
    let ip = guest_access::guest_address(&record.mac_address)?;
    Ok(SocketAddr::from((ip, PORT)))
}

/// Opens a connection to the guest's Screen Sharing port, for the owner to relay.
pub(crate) fn connect(app: &AppHandle, id: &str) -> Result<TcpStream, String> {
    let (_, record) = checked(app, id)?;
    let target = address(&record)?;
    let stream = TcpStream::connect_timeout(&target, CONNECT_TIMEOUT)
        .map_err(|_| SHARING_UNAVAILABLE.to_string())?;
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

/// The sign-in for a running, set-up computer whose Screen Sharing answers.
pub(crate) fn session(app: &AppHandle, id: &str) -> Result<DisplaySession, String> {
    let (layout, record) = checked(app, id)?;
    let target = address(&record)?;
    TcpStream::connect_timeout(&target, CONNECT_TIMEOUT)
        .map_err(|_| SHARING_UNAVAILABLE.to_string())?;
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
    app: &AppHandle,
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

    fn done() -> SetupProgress {
        SetupProgress {
            account: true,
            sip: true,
            computer_use: true,
            clipboard: true,
            needs_personalizing: false,
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
}
