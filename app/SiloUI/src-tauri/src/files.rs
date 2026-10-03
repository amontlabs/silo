//! Read-only, bounded directory snapshots. Pagination never combines two scans.
use crate::runtime::{ensure_managed, run_msb, runtime_paths, ProcessRunner};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tauri::AppHandle;

const PAGE_SIZE: usize = 200;
const MAX_ENTRIES: usize = 20_000;
const MAX_SNAPSHOTS: usize = 64;
const FAILED: &str = "Could not load this folder.";
const EXPIRED: &str = "Folder listing expired. Refresh this folder.";
const TOO_LARGE: &str = "This folder is too large to list.";
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Entry {
    name: String,
    path: String,
    kind: String,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DirectoryPage {
    entries: Vec<Entry>,
    next_offset: Option<usize>,
    snapshot_id: String,
}
static NEXT_SNAPSHOT: AtomicU64 = AtomicU64::new(1);
struct Snapshot {
    computer: String,
    computer_id: String,
    path: String,
    created: Instant,
    entries: Vec<Entry>,
}
/// Keyed by snapshot id so two windows listing the same folder paginate their
/// own scans instead of expiring each other's (G-18).
static SNAPSHOTS: OnceLock<Mutex<HashMap<String, Snapshot>>> = OnceLock::new();

fn cached_page(
    cache: &HashMap<String, Snapshot>,
    computer: &str,
    computer_id: &str,
    path: &str,
    offset: usize,
    snapshot_id: Option<&str>,
) -> Result<DirectoryPage, String> {
    let id = snapshot_id.ok_or(EXPIRED)?;
    let snapshot = cache
        .get(id)
        .filter(|s| {
            s.created.elapsed() < Duration::from_secs(120)
                && s.computer == computer
                && s.computer_id == computer_id
                && s.path == path
        })
        .ok_or(EXPIRED)?;
    page(&snapshot.entries, offset, id)
}

pub(crate) fn valid_path(path: &str) -> bool {
    path.len() <= 4096
        && !path.contains('\0')
        && (path == "/workspace"
            || path.strip_prefix("/workspace/").is_some_and(|tail| {
                tail.split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")
            }))
}
fn parse_listing(output: &str, path: &str) -> Result<Vec<Entry>, String> {
    let mut parts = output.split('\0');
    match parts.next() {
        Some("ok") => (),
        Some("missing") => return Err("This folder no longer exists.".into()),
        Some("denied") => return Err("Permission denied.".into()),
        Some("invalid") => return Err("This folder cannot be browsed.".into()),
        Some("large") => return Err(TOO_LARGE.into()),
        _ => return Err(FAILED.into()),
    }
    let mut entries = Vec::new();
    let mut bytes = 0;
    loop {
        let kind = match parts.next() {
            Some("") if parts.next().is_none() => break,
            Some(kind) => kind,
            None => return Err(FAILED.into()),
        };
        let name = parts.next().ok_or(FAILED)?;
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains('/')
            || entries.len() == MAX_ENTRIES
        {
            return Err(TOO_LARGE.into());
        }
        bytes += name.len() * 2 + path.len() + 96;
        if bytes > 2 * 1024 * 1024 {
            return Err(TOO_LARGE.into());
        }
        let kind = match kind {
            "d" => "folder",
            "l" => "symlink",
            // "u": the guest escaped a name that is not UTF-8. Its path does not
            // exist, so it is listed but never offered as a folder to open.
            "f" | "b" | "c" | "p" | "s" | "u" => "file",
            _ => return Err(FAILED.into()),
        };
        entries.push(Entry {
            name: name.into(),
            path: format!("{path}/{name}"),
            kind: kind.into(),
        });
    }
    entries.sort_by(|a, b| {
        (a.kind != "folder")
            .cmp(&(b.kind != "folder"))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}
fn page(entries: &[Entry], offset: usize, snapshot_id: &str) -> Result<DirectoryPage, String> {
    if offset > entries.len() {
        return Err(EXPIRED.into());
    }
    let end = (offset + PAGE_SIZE).min(entries.len());
    Ok(DirectoryPage {
        entries: entries[offset..end].to_vec(),
        next_offset: (end < entries.len()).then_some(end),
        snapshot_id: snapshot_id.into(),
    })
}
#[tauri::command]
pub(crate) async fn list_computer_directory(
    app: AppHandle,
    computer: String,
    path: String,
    offset: usize,
    snapshot_id: Option<String>,
) -> Result<DirectoryPage, String> {
    if !valid_path(&path) || offset > MAX_ENTRIES || offset % PAGE_SIZE != 0 {
        return Err("Invalid folder request.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        if let Some((device, computer)) = crate::remote_access::target(&computer)? {
            let value = crate::remote::call_remote(
                &app,
                &device,
                "files.list",
                serde_json::json!({
                    "computerId": computer, "path": path, "offset": offset, "snapshotId": snapshot_id,
                }),
            )?;
            return serde_json::from_value(value)
                .map_err(|_| "The remote device returned an invalid folder listing.".into());
        }
        crate::runtime::validate_name(&computer).map_err(|error| error.to_string())?;
        let paths = runtime_paths(&app).map_err(|_| FAILED.to_owned())?;
        let metadata =
            crate::runtime::read_metadata(&paths.metadata).map_err(|_| FAILED.to_owned())?;
        let computer_id = metadata
            .computers
            .iter()
            .find(|configuration| configuration.name() == computer)
            .ok_or("Computer no longer exists.")?
            .id()
            .to_owned();
        let state = match crate::runtime::observe_computer(&ProcessRunner, &paths, &computer)
            .map_err(|_| FAILED.to_owned())?
        {
            crate::runtime::ComputerRuntime::Absent => {
                return Err("Start this computer to browse its files.".into())
            }
            crate::runtime::ComputerRuntime::Present(state) => state,
        };
        ensure_managed(&state).map_err(|_| FAILED.to_owned())?;
        let user = crate::working_account::USER;
        if state.status != "Running" {
            return Err("Start this computer to browse its files.".into());
        }
        let snapshots = SNAPSHOTS.get_or_init(|| Mutex::new(HashMap::new()));
        if offset != 0 {
            let cache = snapshots.lock().map_err(|_| FAILED.to_owned())?;
            return cached_page(
                &cache,
                &computer,
                &computer_id,
                &path,
                offset,
                snapshot_id.as_deref(),
            );
        }
        let output = run_msb(
            &paths,
            &[
                "exec".into(),
                computer.clone(),
                "--user".into(),
                user.into(),
                "--env".into(),
                format!("USER={user}"),
                "--env".into(),
                format!("LOGNAME={user}"),
                "--no-start".into(),
                "--no-tty".into(),
                "--quiet".into(),
                "--timeout".into(),
                "5s".into(),
                "--workdir".into(),
                "/".into(),
                "--".into(),
                "python3".into(),
                "-I".into(),
                "-c".into(),
                include_str!("../guest/list-directory.py").into(),
                path.clone(),
            ],
            Duration::from_secs(8),
        )
        .map_err(|_| FAILED.to_owned())?;
        let entries = parse_listing(&output.stdout, &path)?;
        let id = NEXT_SNAPSHOT.fetch_add(1, Ordering::Relaxed).to_string();
        let result = page(&entries, 0, &id)?;
        let mut cache = snapshots.lock().map_err(|_| FAILED.to_owned())?;
        cache.retain(|_, s| s.created.elapsed() < Duration::from_secs(120));
        while cache.len() >= MAX_SNAPSHOTS
            || cache
                .values()
                .map(|s| {
                    s.entries
                        .iter()
                        .map(|e| e.name.len() + e.path.len() + 96)
                        .sum::<usize>()
                })
                .sum::<usize>()
                > 6 * 1024 * 1024
        {
            if let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, s)| s.created)
                .map(|(key, _)| key.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            id,
            Snapshot {
                computer,
                computer_id,
                path,
                created: Instant::now(),
                entries,
            },
        );
        Ok(result)
    })
    .await
    .map_err(|_| FAILED.to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_reject_escape_and_noncanonical_names() {
        for path in [
            "/",
            "/workspace2",
            "/workspace/../etc",
            "/workspace//x",
            "/workspace/x/",
            "/workspace/./x",
            "/workspace/\0",
        ] {
            assert!(!valid_path(path));
        }
        assert!(valid_path("/workspace/new\nfolder/日本語"));
    }
    #[test]
    fn listing_preserves_names_and_does_not_treat_links_as_folders() {
        let entries = parse_listing("ok\0f\0a\nb\0l\0link\0d\0日本語\0", "/workspace").unwrap();
        assert_eq!(entries[0].kind, "folder");
        assert_eq!(entries[1].name, "a\nb");
        assert_eq!(entries[2].kind, "symlink");
    }
    #[test]
    fn listing_failures_never_become_empty_success() {
        for listing in [
            "",
            "ok",
            "ok\0f\0bad",
            "ok\0d\0../x\0",
            "denied\0",
            "missing\0",
        ] {
            assert!(parse_listing(listing, "/workspace").is_err());
        }
        assert!(parse_listing("ok\0", "/workspace").unwrap().is_empty());
    }
    #[test]
    fn undecodable_names_are_listed_but_never_opened_as_folders() {
        let entries = parse_listing("ok\0u\0caf\\xe9\0d\0real\0", "/workspace").unwrap();
        assert_eq!(entries[0].name, "real");
        assert_eq!(entries[0].kind, "folder");
        assert_eq!(entries[1].name, "caf\\xe9");
        assert_eq!(entries[1].kind, "file");
    }
    #[test]
    fn the_guest_reports_oversized_folders_before_the_output_cap() {
        assert_eq!(
            parse_listing("large\0", "/workspace").unwrap_err(),
            TOO_LARGE
        );
    }
    #[test]
    fn long_paths_cannot_expand_snapshot_memory_without_bound() {
        let output = format!("ok\0{}", "f\0short\0".repeat(1000));
        let path = format!("/workspace/{}", "x".repeat(4000));
        assert_eq!(
            parse_listing(&output, &path).unwrap_err(),
            "This folder is too large to list."
        );
    }
    #[test]
    fn two_windows_listing_one_folder_keep_their_own_snapshots() {
        let snapshot = |name: &str| Snapshot {
            computer: "dev".into(),
            computer_id: "computer-a".into(),
            path: "/workspace".into(),
            created: Instant::now(),
            entries: (0..300)
                .map(|i| Entry {
                    name: format!("{name}{i}"),
                    path: i.to_string(),
                    kind: "file".into(),
                })
                .collect(),
        };
        let cache = HashMap::from([
            ("1".to_string(), snapshot("main")),
            ("2".to_string(), snapshot("status")),
        ]);
        let main = cached_page(&cache, "dev", "computer-a", "/workspace", 200, Some("1")).unwrap();
        let status =
            cached_page(&cache, "dev", "computer-a", "/workspace", 200, Some("2")).unwrap();
        assert_eq!(main.entries[0].name, "main200");
        assert_eq!(status.entries[0].name, "status200");
        assert_eq!(
            cached_page(&cache, "other", "computer-a", "/workspace", 200, Some("1")).unwrap_err(),
            EXPIRED
        );
        assert_eq!(
            cached_page(&cache, "dev", "computer-a", "/workspace/x", 200, Some("1")).unwrap_err(),
            EXPIRED
        );
        assert_eq!(
            cached_page(&cache, "dev", "computer-a", "/workspace", 200, None).unwrap_err(),
            EXPIRED
        );
    }
    #[test]
    fn replacement_computer_with_the_same_name_cannot_page_the_previous_computer_snapshot() {
        let cache = HashMap::from([(
            "snapshot".to_owned(),
            Snapshot {
                computer: "dev".into(),
                computer_id: "computer-a".into(),
                path: "/workspace".into(),
                created: Instant::now(),
                entries: (0..201)
                    .map(|i| Entry {
                        name: format!("previous-computer-{i}"),
                        path: format!("/workspace/previous-computer-{i}"),
                        kind: "file".into(),
                    })
                    .collect(),
            },
        )]);
        assert_eq!(
            cached_page(
                &cache,
                "dev",
                "computer-a",
                "/workspace",
                200,
                Some("snapshot")
            )
            .unwrap()
            .entries[0]
                .name,
            "previous-computer-200"
        );
        assert_eq!(
            cached_page(
                &cache,
                "dev",
                "computer-b",
                "/workspace",
                200,
                Some("snapshot")
            )
            .unwrap_err(),
            EXPIRED
        );
    }
    #[test]
    fn pagination_is_bounded_and_complete() {
        let entries: Vec<_> = (0..401)
            .map(|i| Entry {
                name: i.to_string(),
                path: i.to_string(),
                kind: "file".into(),
            })
            .collect();
        assert_eq!(page(&entries, 0, "test").unwrap().next_offset, Some(200));
        assert_eq!(page(&entries, 200, "test").unwrap().entries.len(), 200);
        assert_eq!(page(&entries, 400, "test").unwrap().entries.len(), 1);
        assert_eq!(page(&entries, 400, "test").unwrap().next_offset, None);
    }
}
