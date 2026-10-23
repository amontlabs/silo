//! Quit requests that come from the operating system rather than Silo's own
//! menus (F-02, F-20; design note "F-02 + F-20" in
//! `docs/SiloUI-REVIEW-DESIGN-NOTES.md`; owner decision 7).
//!
//! - A user Quit (Dock Quit, `osascript quit`) enters the confirm-capable Quit
//!   path (`settings::request_quit`).
//! - Logout, restart, shutdown and SIGTERM never prompt: Silo stops its local
//!   computers within a bound and then exits, even if a stop fails.
//!
//! OS event handling and process exit stay in `commands/system_shutdown`; this
//! module owns the platform-independent routing policy and budgets.
use std::time::Duration;

/// Upper bound for stopping computers when the session ends without a logind limit.
pub const SESSION_END_BUDGET: Duration = Duration::from_secs(20);
/// logind's default `InhibitDelayMaxSec`, used when the property is unreadable.
#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
const LOGIND_DEFAULT_DELAY: Duration = Duration::from_secs(5);
/// Time left after stopping computers to save settings and release the inhibitor.
#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
const LOGIND_MARGIN: Duration = Duration::from_millis(750);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownReason {
    /// The user asked Silo to quit: confirm when computers run.
    UserQuit,
    Logout,
    /// Restart or shutdown.
    Shutdown,
}

/// The Apple event `kAEQuitReason` ('why?') values that end the session. A quit
/// without a reason (Dock Quit, `osascript`) is a user Quit.
#[cfg(any(test, target_os = "macos"))]
pub fn quit_reason(code: Option<u32>) -> ShutdownReason {
    match code.map(u32::to_be_bytes).as_ref() {
        // kAELogOut, kAEReallyLogOut
        Some(b"logo" | b"rlgo") => ShutdownReason::Logout,
        // kAEShowRestartDialog, kAEShowShutdownDialog, kAERestart, kAEShutDown
        Some(b"rrst" | b"rsdn" | b"rest" | b"shut") => ShutdownReason::Shutdown,
        _ => ShutdownReason::UserQuit,
    }
}

/// Stop within logind's delay, leaving a margin to save settings and exit.
#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
pub fn logind_budget(max_delay: Option<Duration>) -> Duration {
    let max_delay = max_delay.unwrap_or(LOGIND_DEFAULT_DELAY);
    let margin = LOGIND_MARGIN.min(max_delay / 2);
    max_delay.saturating_sub(margin).min(SESSION_END_BUDGET)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(value: &[u8; 4]) -> Option<u32> {
        Some(u32::from_be_bytes(*value))
    }

    #[test]
    fn only_session_end_reasons_skip_the_quit_prompt() {
        assert_eq!(quit_reason(None), ShutdownReason::UserQuit);
        assert_eq!(quit_reason(code(b"quia")), ShutdownReason::UserQuit);
        assert_eq!(quit_reason(Some(0)), ShutdownReason::UserQuit);
        for reason in [b"logo", b"rlgo"] {
            assert_eq!(quit_reason(code(reason)), ShutdownReason::Logout);
        }
        for reason in [b"rrst", b"rsdn", b"rest", b"shut"] {
            assert_eq!(quit_reason(code(reason)), ShutdownReason::Shutdown);
        }
    }

    #[test]
    fn logind_budget_leaves_time_to_exit_inside_the_delay() {
        assert_eq!(logind_budget(None), Duration::from_millis(4250));
        assert_eq!(
            logind_budget(Some(Duration::from_secs(30))),
            Duration::from_millis(29250).min(SESSION_END_BUDGET)
        );
        assert_eq!(
            logind_budget(Some(Duration::from_millis(500))),
            Duration::from_millis(250)
        );
        assert_eq!(
            logind_budget(Some(Duration::from_secs(10))),
            Duration::from_millis(9250)
        );
    }

    #[test]
    fn short_logind_limits_are_not_extended_by_a_minimum_computer_budget() {
        for limit in [
            Duration::from_micros(1),
            Duration::from_millis(500),
            Duration::from_secs(1),
            Duration::from_millis(1200),
        ] {
            let budget = logind_budget(Some(limit));
            assert!(
                budget < limit,
                "shutdown must leave time to release the inhibitor"
            );
            assert!(
                !budget.is_zero(),
                "positive delay must leave time to try stopping computers"
            );
        }
        assert_eq!(logind_budget(Some(Duration::ZERO)), Duration::ZERO);
    }
}
