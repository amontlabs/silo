use std::time::{Duration, SystemTime};

pub(super) const POLL_INTERVAL: Duration = Duration::from_secs(5);
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Wall time includes suspend on Linux. Polling avoids a day-long sleep hiding
/// overdue checks after resume, preference changes, or a skipped busy operation.
pub(super) struct Schedule {
    next_check: SystemTime,
    failures: u32,
    last_check: SystemTime,
}
impl Schedule {
    pub(super) fn new(now: SystemTime) -> Self {
        Self {
            next_check: now + POLL_INTERVAL,
            failures: 0,
            last_check: now,
        }
    }
    pub(super) fn enable(&mut self, now: SystemTime) {
        self.next_check = now;
        self.failures = 0;
    }
    pub(super) fn focus(&mut self, now: SystemTime) {
        // Focus must not flood requests or defeat offline retry backoff.
        if self.failures == 0
            && now.duration_since(self.last_check).unwrap_or_default() >= Duration::from_secs(60)
        {
            self.next_check = now;
        }
    }
    /// `pending`: a verified download or a download/install retry must not be replaced.
    pub(super) fn due(&self, now: SystemTime, enabled: bool, busy: bool, pending: bool) -> bool {
        enabled && !busy && !pending && now >= self.next_check
    }
    pub(super) fn completed(&mut self, now: SystemTime, success: bool) {
        self.last_check = now;
        let delay = if success {
            self.failures = 0;
            CHECK_INTERVAL
        } else {
            self.failures = self.failures.saturating_add(1);
            Duration::from_secs((60 * 2u64.pow(self.failures.min(5) - 1)).min(15 * 60))
        };
        self.next_check = now + delay;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const START: SystemTime = SystemTime::UNIX_EPOCH;
    fn due(schedule: &Schedule, seconds: u64) -> bool {
        schedule.due(START + Duration::from_secs(seconds), true, false, false)
    }
    #[test]
    fn focus_checks_after_cooldown_without_discarding_retry_backoff() {
        let mut schedule = Schedule::new(START);
        schedule.completed(START, true);
        schedule.focus(START + Duration::from_secs(59));
        assert!(!due(&schedule, 59));
        schedule.focus(START + Duration::from_secs(60));
        assert!(due(&schedule, 60));
        assert!(!schedule.due(START + Duration::from_secs(60), false, false, false));
        assert!(!schedule.due(START + Duration::from_secs(60), true, false, true));
        schedule.completed(START, false);
        schedule.completed(START, false);
        schedule.focus(START + Duration::from_secs(60));
        assert!(!due(&schedule, 60));
        assert!(due(&schedule, 120));
    }
    #[test]
    fn launch_discovers_without_manual_action() {
        let schedule = Schedule::new(START);
        assert!(!due(&schedule, 0));
        assert!(due(&schedule, 5));
    }
    #[test]
    fn offline_launch_retries_in_a_minute_then_backs_off_and_recovers() {
        let mut schedule = Schedule::new(START);
        schedule.completed(START, false);
        assert!(!due(&schedule, 59));
        assert!(due(&schedule, 60));
        schedule.completed(START, false);
        assert!(!due(&schedule, 119));
        assert!(due(&schedule, 120));
        for _ in 0..100 {
            schedule.completed(START, false);
        }
        assert!(!due(&schedule, 899));
        assert!(due(&schedule, 900));
        schedule.completed(START, true);
        assert!(!due(&schedule, 86399));
        assert!(due(&schedule, 86400));
        schedule.completed(START, false);
        assert!(due(&schedule, 60));
    }
    #[test]
    fn reenable_checks_promptly_even_after_recent_success() {
        let mut schedule = Schedule::new(START);
        schedule.completed(START, true);
        assert!(!schedule.due(START + CHECK_INTERVAL, false, false, false));
        schedule.enable(START);
        assert!(due(&schedule, 0));
    }
    #[test]
    fn busy_skip_does_not_postpone_check_and_pending_updates_are_preserved() {
        let schedule = Schedule::new(START);
        let now = START + POLL_INTERVAL;
        assert!(!schedule.due(now, true, true, false));
        assert!(schedule.due(now, true, false, false));
        assert!(!schedule.due(now + CHECK_INTERVAL, true, false, true));
    }
    #[test]
    fn resume_after_a_day_is_due_without_waiting_another_day() {
        let mut schedule = Schedule::new(START);
        schedule.completed(START, true);
        assert!(due(&schedule, 2 * 86400));
    }
}
