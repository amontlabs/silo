//! Stream retained log pages to an atomic, user-selected JSON Lines export.
use crate::runtime::runtime_logs::{Page, Query};
use serde::Serialize;
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, MutexGuard,
    },
};

static EXPORT_LOCK: Mutex<()> = Mutex::new(());
static CANCELLATION: AtomicU64 = AtomicU64::new(0);

pub fn cancellation_check() -> impl Fn() -> bool {
    let generation = CANCELLATION.load(Ordering::Acquire);
    move || CANCELLATION.load(Ordering::Acquire) != generation
}

pub fn cancel() {
    CANCELLATION.fetch_add(1, Ordering::Release);
}

pub struct ExportPermit {
    _guard: MutexGuard<'static, ()>,
}

/// Serializes the complete export flow, including the destination picker.
pub fn begin_export() -> Result<ExportPermit, String> {
    let guard = crate::sync::try_lock_or_recover(&EXPORT_LOCK, "log export")
        .ok_or("A log export is already in progress.")?;
    Ok(ExportPermit { _guard: guard })
}

pub fn validate_requests(requests: &[Query]) -> Result<(), String> {
    if requests.is_empty() || requests.len() > 100 {
        return Err("Select between one and 100 computers to export logs.".into());
    }
    Ok(())
}

fn write_json_line(output: &mut impl Write, value: &impl Serialize) -> Result<(), String> {
    serde_json::to_writer(&mut *output, value).map_err(|_| "Could not write the log export.")?;
    output
        .write_all(b"\n")
        .map_err(|_| "Could not write the log export.".into())
}

// The selected destination is replaced only after every page succeeds. A dropped
// temporary removes partial output on cancellation, disconnect or disk failure.
fn save_atomically(
    destination: &Path,
    write: impl FnOnce(&mut std::fs::File) -> Result<bool, String>,
    cancelled: impl Fn() -> bool,
) -> Result<bool, String> {
    let parent = destination
        .parent()
        .ok_or("The export destination is unavailable.")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "Could not create the log export at this destination.")?;
    if !write(temporary.as_file_mut())? {
        return Ok(false);
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| "Could not finish the log export.")?;
    if cancelled() {
        return Ok(false);
    }
    temporary
        .persist(destination)
        .map_err(|_| "Could not save the completed log export.")?;
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| "Could not save the completed log export.")?;
    Ok(true)
}

pub fn export_to(
    _permit: ExportPermit,
    requests: Vec<Query>,
    destination: PathBuf,
    query: impl FnMut(Query) -> Result<Page, String>,
    cancelled: impl Fn() -> bool,
) -> Result<bool, String> {
    validate_requests(&requests)?;
    if cancelled() {
        return Ok(false);
    }
    save_atomically(
        &destination,
        |output| write_requests(output, requests, query, &cancelled),
        &cancelled,
    )
}

#[cfg(test)]
fn export_with(
    requests: Vec<Query>,
    select: impl FnOnce() -> Result<Option<PathBuf>, String>,
    query: impl FnMut(Query) -> Result<Page, String>,
    cancelled: impl Fn() -> bool,
) -> Result<bool, String> {
    validate_requests(&requests)?;
    let permit = begin_export()?;
    if cancelled() {
        return Ok(false);
    }
    let Some(destination) = select()? else {
        return Ok(false);
    };
    if cancelled() {
        return Ok(false);
    }
    export_to(permit, requests, destination, query, cancelled)
}

pub(crate) fn write_requests(
    output: &mut impl Write,
    requests: Vec<Query>,
    mut query: impl FnMut(Query) -> Result<Page, String>,
    cancelled: impl Fn() -> bool,
) -> Result<bool, String> {
    write_json_line(
        output,
        &serde_json::json!({
            "type": "silo-log-export", "version": 1,
            "note": "Each computer is a separate snapshot. Timestamps and identities are retained. Silo hides known secret values, but logs can still contain sensitive output; review it before sharing."
        }),
    )?;
    for mut request in requests {
        // Export all matches, including pages the user has not loaded.
        request.cursor = None;
        let mut cursors = std::collections::HashSet::new();
        loop {
            if cancelled() {
                return Ok(false);
            }
            let page = query(request.clone());
            if cancelled() {
                return Ok(false);
            }
            let page = page?;
            if page.unsupported {
                return Err("Update Silo on the remote device before exporting its logs.".into());
            }
            if request.cursor.is_none() {
                let mut coverage_request = request.clone();
                coverage_request.query = request
                    .query
                    .as_ref()
                    .map(|_| "[Search text hidden]".into());
                write_json_line(
                    output,
                    &serde_json::json!({
                        "type": "coverage", "request": coverage_request,
                        "oldestAvailableTimestamp": page.oldest_available_timestamp,
                        "newestAvailableTimestamp": page.newest_available_timestamp,
                        "totalMatches": page.total_matches,
                        "timestampEstimated": page.timestamp_estimated,
                        "unreadableRecords": page.unreadable_records,
                    }),
                )?;
            }
            if page.next_cursor.is_some() && page.entries.is_empty() {
                return Err("The log export returned an incomplete page. Retry the export.".into());
            }
            for entry in &page.entries {
                write_json_line(output, entry)?;
            }
            match page.next_cursor {
                Some(cursor) if cursors.insert(cursor.clone()) => request.cursor = Some(cursor),
                Some(_) => return Err("The log export stopped advancing. Retry the export.".into()),
                None => break,
            }
        }
    }
    Ok(!cancelled())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::runtime_logs;

    #[cfg(unix)]
    #[test]
    fn completed_export_reports_an_unreadable_parent_after_publication() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let directory = tempfile::tempdir().unwrap();
        if std::fs::metadata(directory.path()).unwrap().uid() == 0 {
            return; // Root bypasses the permission boundary exercised here.
        }
        let destination = directory.path().join("logs.jsonl");
        std::fs::write(&destination, b"previous export").unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o300)).unwrap();
        let result = save_atomically(
            &destination,
            |output| {
                output
                    .write_all(b"complete export")
                    .map_err(|error| error.to_string())?;
                Ok(true)
            },
            || false,
        );
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"complete export");
        assert!(
            result.is_err(),
            "an unsynchronized rename must not report success"
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn cancellation_before_the_export_worker_starts_skips_the_picker_and_output() {
        let _isolation = crate::test_support::global_state();
        let cancelled = cancellation_check();
        cancel();
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("logs.jsonl");
        std::fs::write(&destination, b"previous export").unwrap();
        let selected = std::cell::Cell::new(false);
        let saved = export_with(
            vec![Query::default()],
            || {
                selected.set(true);
                Ok(Some(destination.clone()))
            },
            |_| Ok(page(0, 1)),
            cancelled,
        )
        .unwrap();
        assert!(!saved);
        assert!(!selected.get());
        assert_eq!(std::fs::read(&destination).unwrap(), b"previous export");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_new_export_after_cancellation_can_save_normally() {
        let _isolation = crate::test_support::global_state();
        let old = cancellation_check();
        cancel();
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("logs.jsonl");
        assert!(export_with(
            vec![Query::default()],
            || Ok(Some(destination.clone())),
            |_| Ok(page(0, 1)),
            cancellation_check(),
        )
        .unwrap());
        assert!(
            old(),
            "starting another export must not undo the old cancellation"
        );
        assert_eq!(
            std::fs::read_to_string(destination)
                .unwrap()
                .lines()
                .count(),
            3
        );
    }

    #[test]
    fn a_panicked_export_does_not_block_the_next_export() {
        let _isolation = crate::test_support::global_state();
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("logs.jsonl");
        std::fs::write(&destination, b"previous export").unwrap();
        let failure = std::panic::catch_unwind(|| {
            export_with(
                vec![Query::default()],
                || Ok(Some(destination.clone())),
                |_| panic!("simulated log-query panic"),
                || false,
            )
        });
        assert!(failure.is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"previous export");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        let retry = export_with(
            vec![Query::default()],
            || Ok(Some(destination.clone())),
            |_| Ok(page(0, 1)),
            || false,
        );
        let remained_poisoned = EXPORT_LOCK.is_poisoned();
        EXPORT_LOCK.clear_poison();
        assert!(retry.unwrap());
        assert!(!remained_poisoned);
    }

    fn page(offset: usize, total: usize) -> Page {
        let end = (offset + 200).min(total);
        Page {
            entries: (offset..end)
                .map(|index| runtime_logs::Entry {
                    id: index.to_string(),
                    line: format!("échec\nrecord {index}"),
                    occurred_at: "2026-09-18T10:00:00Z".into(),
                    computer_id: "computer-id".into(),
                    computer_name: "dev".into(),
                    device_id: "host-id".into(),
                    device_name: "Build device".into(),
                    source: "stderr".into(),
                    session: Some("42".into()),
                    guest_timestamp: false,
                })
                .collect(),
            next_cursor: (end < total).then(|| end.to_string()),
            oldest_available_timestamp: Some("2026-09-18T10:00:00Z".into()),
            newest_available_timestamp: Some("2026-09-18T10:00:00Z".into()),
            total_matches: total,
            timestamp_estimated: false,
            unsupported: false,
            unreadable_records: false,
            snapshot: None,
        }
    }

    #[test]
    fn export_reads_every_matching_page_and_preserves_identity() {
        let mut output = Vec::new();
        let mut calls = 0;
        let query = Query {
            computer_id: "computer-id".into(),
            cursor: Some("400".into()),
            ..Query::default()
        };
        assert!(write_requests(
            &mut output,
            vec![query],
            |request| {
                calls += 1;
                Ok(page(
                    request.cursor.as_deref().unwrap_or("0").parse().unwrap(),
                    1001,
                ))
            },
            || false
        )
        .unwrap());
        assert_eq!(calls, 6);
        let text = String::from_utf8(output).unwrap();
        let records: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records.len(), 1003); // Header, coverage, then all records.
        assert_eq!(records[2]["id"], "0");
        assert_eq!(records.last().unwrap()["id"], "1000");
        assert_eq!(records.last().unwrap()["deviceName"], "Build device");
        assert_eq!(records.last().unwrap()["session"], "42");
        assert_eq!(records.last().unwrap()["line"], "échec\nrecord 1000");
    }

    #[test]
    fn export_hides_search_text_but_preserves_filtering_and_coverage() {
        for text in [
            "ghp_synthetic_export_search_secret",
            "private customer lookup",
        ] {
            let request = Query {
                computer_id: "computer-id".into(),
                query: Some(text.into()),
                source: Some("stderr".into()),
                ..Query::default()
            };
            let mut output = Vec::new();
            assert!(write_requests(
                &mut output,
                vec![request],
                |request| {
                    assert_eq!(request.query.as_deref(), Some(text));
                    Ok(page(0, 0))
                },
                || false,
            )
            .unwrap());
            let output = String::from_utf8(output).unwrap();
            assert!(
                !output.contains(text),
                "Search input must stay out of shared exports"
            );
            let coverage: serde_json::Value =
                serde_json::from_str(output.lines().nth(1).unwrap()).unwrap();
            assert_eq!(coverage["request"]["computerId"], "computer-id");
            assert_eq!(coverage["request"]["source"], "stderr");
            assert_eq!(coverage["request"]["query"], "[Search text hidden]");
            assert_eq!(coverage["totalMatches"], 0);
        }
    }

    #[test]
    fn cancellation_stops_after_pending_page_without_publishing_it() {
        let cancelled = std::cell::Cell::new(false);
        let mut output = Vec::new();
        let mut calls = 0;
        let result = write_requests(
            &mut output,
            vec![Query::default()],
            |_| {
                calls += 1;
                cancelled.set(true);
                Ok(page(0, 1001))
            },
            || cancelled.get(),
        )
        .unwrap();
        assert!(!result);
        assert_eq!(calls, 1);
        assert_eq!(String::from_utf8(output).unwrap().lines().count(), 1);
    }

    #[test]
    fn cancellation_wins_over_a_pending_page_failure_without_hiding_other_errors() {
        for cancel in [false, true] {
            let cancelled = std::cell::Cell::new(false);
            let mut output = Vec::new();
            let result = write_requests(
                &mut output,
                vec![Query::default()],
                |_| {
                    cancelled.set(cancel);
                    Err("Remote device disconnected.".into())
                },
                || cancelled.get(),
            );
            if cancel {
                assert!(!result.unwrap());
            } else {
                assert_eq!(result.unwrap_err(), "Remote device disconnected.");
            }
            assert_eq!(String::from_utf8(output).unwrap().lines().count(), 1);
        }
    }

    #[test]
    fn export_header_does_not_promise_that_sensitive_output_was_removed() {
        let mut output = Vec::new();
        write_requests(&mut output, vec![], |_| Ok(page(0, 0)), || false).unwrap();
        let header = String::from_utf8(output).unwrap();
        assert!(!header.contains("sensitive output is filtered"));
        assert!(header.contains("review it before sharing"));
    }

    #[test]
    fn repeated_remote_cursor_fails_instead_of_looping_forever() {
        let mut output = Vec::new();
        let result = write_requests(
            &mut output,
            vec![Query::default()],
            |_| Ok(page(0, 1001)),
            || false,
        );
        assert!(result.unwrap_err().contains("stopped advancing"));
    }

    #[test]
    fn unsupported_remote_logs_do_not_replace_an_existing_export() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("logs.jsonl");
        std::fs::write(&destination, b"previous complete export").unwrap();
        let requests = vec![
            Query::default(),
            Query {
                device_id: Some("older-device".into()),
                ..Query::default()
            },
        ];
        let result = save_atomically(
            &destination,
            |output| {
                write_requests(
                    output,
                    requests,
                    |request| {
                        let mut response = page(0, usize::from(request.device_id.is_none()));
                        response.unsupported = request.device_id.is_some();
                        Ok(response)
                    },
                    || false,
                )
            },
            || false,
        );
        assert!(result.unwrap_err().contains("Update Silo"));
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"previous complete export"
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_or_cancelled_export_preserves_existing_destination_and_removes_partial_file() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("logs.jsonl");
        std::fs::write(&destination, b"previous export").unwrap();
        let failed = save_atomically(
            &destination,
            |output| {
                output.write_all(b"partial page").unwrap();
                Err("Remote device disconnected.".into())
            },
            || false,
        );
        assert!(failed.is_err());
        assert!(!save_atomically(
            &destination,
            |output| {
                output.write_all(b"cancelled page").unwrap();
                Ok(false)
            },
            || false
        )
        .unwrap());
        assert_eq!(std::fs::read(&destination).unwrap(), b"previous export");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn cancellation_after_writing_preserves_the_previous_export() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("logs.jsonl");
        std::fs::write(&destination, b"previous export").unwrap();
        let cancelled = std::cell::Cell::new(false);
        let saved = save_atomically(
            &destination,
            |output| {
                let complete = write_requests(
                    output,
                    vec![Query::default()],
                    |_| Ok(page(0, 1)),
                    || cancelled.get(),
                )?;
                assert!(complete);
                cancelled.set(true);
                Ok(complete)
            },
            || cancelled.get(),
        )
        .unwrap();
        assert!(!saved);
        assert_eq!(std::fs::read(&destination).unwrap(), b"previous export");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn completed_export_preserves_record_boundaries_and_unicode() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("logs.jsonl");
        assert!(save_atomically(&destination, |output| {
            for index in 0..1001 {
                write_json_line(output, &serde_json::json!({"id": index, "line": "échec\nsecond line", "computerId": "computer-id", "deviceId": "host-id", "occurredAt": "2026-09-18T10:00:00Z"}))?;
            }
            Ok(true)
        }, || false).unwrap());
        let text = std::fs::read_to_string(destination).unwrap();
        assert_eq!(text.lines().count(), 1001);
        let last: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
        assert_eq!(last["id"], 1000);
        assert_eq!(last["line"], "échec\nsecond line");
        assert_eq!(last["deviceId"], "host-id");
    }
}
