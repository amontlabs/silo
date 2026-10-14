//! The pure part of automatic-checkpoint retention (95 A-18): which of a computer's
//! records fall outside the newest N automatic ones. Executing a plan is platform work
//! that goes through each platform's ordinary safe deletion.

use time::format_description::well_known::Rfc3339;

/// One stored checkpoint, as retention needs to see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    /// Creation time in milliseconds since the Unix epoch.
    pub created_at_ms: i128,
    /// Policy kind `automatic`. Manual and before-restore records are never candidates.
    pub automatic: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The newest automatic records, kept whatever happens to older ones.
    pub keep: Vec<String>,
    /// Automatic records beyond the newest N, oldest first.
    pub candidates: Vec<String>,
}

/// Sorts automatic records newest first (checkpoint id breaks ties deterministically) and
/// keeps `keep_newest` of them. A count of zero means the policy is off: nothing is a
/// candidate, so no existing checkpoint is ever removed by switching it off.
pub fn plan(entries: &[Entry], keep_newest: u32) -> Plan {
    let mut automatic: Vec<&Entry> = entries.iter().filter(|entry| entry.automatic).collect();
    automatic.sort_by(|left, right| {
        right
            .created_at_ms
            .cmp(&left.created_at_ms)
            .then_with(|| right.id.cmp(&left.id))
    });
    if keep_newest == 0 {
        return Plan {
            keep: automatic.iter().map(|entry| entry.id.clone()).collect(),
            candidates: Vec::new(),
        };
    }
    let split = (keep_newest as usize).min(automatic.len());
    let (kept, excess) = automatic.split_at(split);
    Plan {
        keep: kept.iter().map(|entry| entry.id.clone()).collect(),
        candidates: excess.iter().rev().map(|entry| entry.id.clone()).collect(),
    }
}

/// A canonical ISO 8601 timestamp for a Linux millisecond date.
pub fn format_millis(millis: u64) -> String {
    time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * 1_000_000)
        .ok()
        .and_then(|time| time.format(&Rfc3339).ok())
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_owned())
}

/// Milliseconds since the Unix epoch for a stored ISO 8601 date; unreadable dates sort oldest.
pub fn parse_millis(text: &str) -> i128 {
    time::OffsetDateTime::parse(text, &Rfc3339)
        .map(|time| time.unix_timestamp_nanos() / 1_000_000)
        .unwrap_or(i128::MIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, created_at_ms: i128, automatic: bool) -> Entry {
        Entry {
            id: id.into(),
            created_at_ms,
            automatic,
        }
    }

    #[test]
    fn keeps_the_newest_automatic_records_and_never_selects_manual_ones() {
        let entries = [
            entry("a1", 10, true),
            entry("m1", 5, false),
            entry("a2", 20, true),
            entry("a3", 30, true),
            entry("a4", 40, true),
            entry("m2", 1, false),
        ];
        let plan = plan(&entries, 3);
        assert_eq!(plan.keep, ["a4", "a3", "a2"]);
        assert_eq!(plan.candidates, ["a1"]);
    }

    #[test]
    fn candidates_run_oldest_first_and_ties_break_by_id() {
        let entries = [
            entry("b", 10, true),
            entry("a", 10, true),
            entry("c", 20, true),
            entry("d", 30, true),
        ];
        let plan = plan(&entries, 1);
        assert_eq!(plan.keep, ["d"]);
        assert_eq!(plan.candidates, ["a", "b", "c"]);
    }

    #[test]
    fn zero_turns_retention_off_without_selecting_anything() {
        let entries = [entry("a1", 1, true), entry("a2", 2, true)];
        let plan = plan(&entries, 0);
        assert!(plan.candidates.is_empty());
        assert_eq!(plan.keep.len(), 2);
    }

    #[test]
    fn fewer_records_than_the_count_selects_nothing() {
        assert!(plan(&[entry("a1", 1, true)], 3).candidates.is_empty());
    }

    #[test]
    fn dates_round_trip_through_the_canonical_form() {
        let text = format_millis(1_760_000_000_123);
        assert_eq!(text, "2025-10-09T08:53:20.123Z");
        assert_eq!(parse_millis(&text), 1_760_000_000_123);
        assert_eq!(parse_millis("not a date"), i128::MIN);
    }
}
