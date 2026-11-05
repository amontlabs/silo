//! Times as the engine writes them (RFC 3339), and the local clock time shown to people.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Seconds since the epoch of `2026-11-05T14:02:31Z`, `...31.123Z` or `...31+01:00`.
pub fn parse_rfc3339(text: &str) -> Option<i64> {
    let bytes = text.trim().as_bytes();
    let number = |from: usize, len: usize| -> Option<i64> {
        let digits = bytes.get(from..from + len)?;
        digits
            .iter()
            .all(u8::is_ascii_digit)
            .then(|| std::str::from_utf8(digits).ok()?.parse().ok())
            .flatten()
    };
    if bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || !matches!(bytes.get(10), Some(b'T' | b't' | b' '))
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return None;
    }
    let (year, month, day) = (number(0, 4)?, number(5, 2)?, number(8, 2)?);
    let (hour, minute, second) = (number(11, 2)?, number(14, 2)?, number(17, 2)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    let mut at = 19;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
    }
    let offset = match bytes.get(at) {
        Some(b'Z' | b'z') if at + 1 == bytes.len() => 0,
        Some(sign @ (b'+' | b'-')) if bytes.get(at + 3) == Some(&b':') && at + 6 == bytes.len() => {
            let seconds = number(at + 1, 2)? * 3600 + number(at + 4, 2)? * 60;
            if *sign == b'+' {
                seconds
            } else {
                -seconds
            }
        }
        _ => return None,
    };
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

/// `2026-11-05T14:02:31Z` for seconds since the epoch.
pub fn format_rfc3339_utc(unix: i64) -> String {
    let (year, month, day) = civil_from_days(unix.div_euclid(86_400));
    let seconds = unix.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

/// `14:02:31` on this computer's clock.
pub fn local_clock(unix: i64) -> String {
    let time = unix as libc::time_t;
    // SAFETY: `localtime_r` writes only to the zeroed `tm` it is given.
    let local = unsafe {
        let mut local: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&time, &mut local).is_null() {
            return format!(
                "{:02}:{:02}:{:02} UTC",
                unix.rem_euclid(86_400) / 3600,
                unix.rem_euclid(3600) / 60,
                unix.rem_euclid(60)
            );
        }
        local
    };
    format!(
        "{:02}:{:02}:{:02}",
        local.tm_hour, local.tm_min, local.tm_sec
    )
}

/// `45s`, `3m 10s` or `2h 5m`.
pub fn age_text(seconds: i64) -> String {
    let seconds = seconds.max(0);
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}

/// A duration such as `90`, `90s`, `5m` or `1h`, in seconds.
pub fn parse_duration_secs(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let (digits, unit) = match text.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((at, _)) => text.split_at(at),
        None => (text, "s"),
    };
    let amount: u64 = digits
        .parse()
        .map_err(|_| format!("`{text}` is not a duration (use 90s, 5m or 1h)"))?;
    let factor = match unit.trim() {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        _ => return Err(format!("`{text}` is not a duration (use 90s, 5m or 1h)")),
    };
    amount
        .checked_mul(factor)
        .ok_or_else(|| format!("`{text}` is too long"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_round_trips_and_honours_offsets() {
        let at = parse_rfc3339("2026-11-05T14:02:31Z").unwrap();
        assert_eq!(format_rfc3339_utc(at), "2026-11-05T14:02:31Z");
        assert_eq!(parse_rfc3339("2026-11-05T15:02:31.250+01:00"), Some(at));
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2024-02-29T00:00:00Z"), Some(1_709_164_800));
        assert_eq!(parse_rfc3339("t2"), None);
        assert_eq!(parse_rfc3339("2026-13-05T14:02:31Z"), None);
        assert_eq!(parse_rfc3339("2026-11-05T14:02:31"), None);
    }

    #[test]
    fn ages_and_durations_read_naturally() {
        assert_eq!(age_text(5), "5s");
        assert_eq!(age_text(190), "3m 10s");
        assert_eq!(age_text(7500), "2h 5m");
        assert_eq!(age_text(-4), "0s");
        assert_eq!(parse_duration_secs("5m"), Ok(300));
        assert_eq!(parse_duration_secs("90"), Ok(90));
        assert_eq!(parse_duration_secs("1h"), Ok(3600));
        assert!(parse_duration_secs("soon").is_err());
        assert!(parse_duration_secs("5d").is_err());
        assert!(parse_duration_secs("").is_err());
    }

    #[test]
    fn the_local_clock_has_the_shape_of_a_time() {
        let text = local_clock(1_788_000_000);
        assert_eq!(text.len(), 8, "{text}");
        assert_eq!(text.matches(':').count(), 2);
    }
}
