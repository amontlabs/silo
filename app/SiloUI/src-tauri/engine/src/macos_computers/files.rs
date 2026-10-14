//! Bounded Files browsing for macOS guests. Directory names are base64 encoded in the
//! guest before SSH writes them, so newlines and non-UTF-8 names cannot corrupt records.
use super::{guest_access, MacosContext};
use crate::files::{DirectoryPage, Entry};
use base64::Engine as _;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

pub(crate) const WORKSPACE: &str = "/Users/silo/workspace";
pub(crate) const HOME: &str = "/Users/silo";
pub(crate) const DOWNLOADS: &str = "/Users/silo/Downloads";
const PAGE_SIZE: usize = 200;
const MAX_ENTRIES: usize = 20_000;
const MAX_SNAPSHOTS: usize = 64;
const MAX_SNAPSHOT_BYTES: usize = 6 * 1024 * 1024;
const MAX_LISTING_BYTES: usize = 1024 * 1024;
const TOO_LARGE: &str = "This folder is too large to list.";
const EXPIRED: &str = "Folder listing expired. Refresh this folder.";
const FAILED: &str = "Could not load this folder.";
const GUEST_LIST: &str = r#"
set -o pipefail
root=$1
if [ ! -d "$root" ]; then printf 'missing\0'; exit 0; fi
if [ ! -r "$root" ]; then printf 'denied\0'; exit 0; fi
expected=$root
physical=$(cd "$root" 2>/dev/null && pwd -P) || { printf 'missing\0'; exit 0; }
case "$physical" in
  /Users/silo/workspace|/Users/silo/workspace/*) ;;
  *) printf 'denied\0'; exit 0 ;;
esac
if [ "$physical" != "$expected" ]; then printf 'denied\0'; exit 0; fi
root=$physical
printf 'ok\0'
/usr/bin/find "$root" -mindepth 1 -maxdepth 1 -print0 |
while IFS= read -r -d '' entry; do
  name=${entry##*/}
  if [ -L "$entry" ]; then
    kind=l
  else
    type=$(/usr/bin/stat -f %HT "$entry" 2>/dev/null) || exit 1
    case "$type" in
      Directory) kind=d ;;
      *) kind=f ;;
    esac
  fi
  encoded=$(/usr/bin/printf '%s' "$name" | /usr/bin/base64 | /usr/bin/tr -d '\n') || exit 1
  /usr/bin/printf '%s\0%s\0' "$kind" "$encoded"
done
"#;

#[derive(Clone)]
struct Snapshot {
    computer_id: String,
    generation: u64,
    caller: String,
    path: String,
    created: Instant,
    entries: Vec<Entry>,
}

fn snapshots() -> &'static Mutex<HashMap<String, Snapshot>> {
    static SNAPSHOTS: OnceLock<Mutex<HashMap<String, Snapshot>>> = OnceLock::new();
    SNAPSHOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed).to_string()
}

pub(crate) fn valid_path(path: &str) -> bool {
    path.len() <= 4096
        && !path.bytes().any(|byte| byte.is_ascii_control())
        && (path == WORKSPACE
            || path
                .strip_prefix("/Users/silo/workspace/")
                .is_some_and(|tail| {
                    tail.split('/')
                        .all(|part| !part.is_empty() && part != "." && part != "..")
                }))
}

pub(crate) fn transfer_roots() -> Vec<String> {
    vec![WORKSPACE.to_string(), format!("{HOME}/Downloads")]
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn guest_command(path: &str) -> String {
    format!(
        "/bin/bash -c {} silo-list {}",
        shell_quote(GUEST_LIST),
        shell_quote(path)
    )
}

fn parse_listing(output: &str, path: &str) -> Result<Vec<Entry>, String> {
    if output.len() > MAX_LISTING_BYTES {
        return Err(TOO_LARGE.into());
    }
    let mut fields = output.split('\0');
    match fields.next() {
        Some("ok") => {}
        Some("missing") => return Err("This folder no longer exists.".into()),
        Some("denied") => return Err("Permission denied.".into()),
        _ => return Err(FAILED.into()),
    }
    let mut entries = Vec::new();
    let mut bytes = 0usize;
    loop {
        let kind = match fields.next() {
            Some("") if fields.next().is_none() => break,
            Some(kind) => kind,
            None => return Err(FAILED.into()),
        };
        let encoded = fields.next().ok_or(FAILED)?;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| FAILED)?;
        if raw.is_empty() || raw == b"." || raw == b".." || raw.contains(&b'/') {
            return Err(TOO_LARGE.into());
        }
        if entries.len() == MAX_ENTRIES {
            return Err(TOO_LARGE.into());
        }
        let name = std::str::from_utf8(&raw)
            .map(str::to_owned)
            .map_err(|_| "This folder contains a file name Silo cannot display.")?;
        let kind = match kind {
            "d" => "folder",
            "l" => "symlink",
            "f" => "file",
            _ => return Err(FAILED.into()),
        };
        bytes = bytes.saturating_add(name.len().saturating_mul(2) + path.len() + 96);
        if bytes > MAX_LISTING_BYTES {
            return Err(TOO_LARGE.into());
        }
        entries.push(Entry {
            path: format!("{path}/{name}"),
            name,
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
        snapshot_id: snapshot_id.to_string(),
    })
}

fn cached_page(
    computer_id: &str,
    generation: u64,
    caller: &str,
    path: &str,
    offset: usize,
    snapshot_id: Option<&str>,
) -> Result<DirectoryPage, String> {
    let id = snapshot_id.ok_or(EXPIRED)?;
    let cache = snapshots().lock().map_err(|_| FAILED)?;
    let snapshot = cache
        .get(id)
        .filter(|snapshot| {
            snapshot.created.elapsed() < Duration::from_secs(120)
                && snapshot.computer_id == computer_id
                && snapshot.generation == generation
                && snapshot.caller == caller
                && snapshot.path == path
        })
        .ok_or(EXPIRED)?;
    page(&snapshot.entries, offset, id)
}

fn insert_snapshot(
    id: &str,
    computer_id: &str,
    generation: u64,
    caller: &str,
    path: &str,
    entries: Vec<Entry>,
) -> Result<(), String> {
    let mut cache = snapshots().lock().map_err(|_| FAILED)?;
    cache.retain(|_, snapshot| snapshot.created.elapsed() < Duration::from_secs(120));
    let new_bytes = entries
        .iter()
        .map(|entry| entry.name.len() + entry.path.len() + 96)
        .sum::<usize>();
    while cache.len() >= MAX_SNAPSHOTS
        || cache
            .values()
            .map(|snapshot| {
                snapshot
                    .entries
                    .iter()
                    .map(|entry| entry.name.len() + entry.path.len() + 96)
                    .sum::<usize>()
            })
            .sum::<usize>()
            + new_bytes
            > MAX_SNAPSHOT_BYTES
    {
        let oldest = cache
            .iter()
            .min_by_key(|(_, snapshot)| snapshot.created)
            .map(|(id, _)| id.clone());
        match oldest {
            Some(oldest) => {
                cache.remove(&oldest);
            }
            None => return Err(TOO_LARGE.into()),
        }
    }
    cache.insert(
        id.into(),
        Snapshot {
            computer_id: computer_id.into(),
            generation,
            caller: caller.into(),
            path: path.into(),
            created: Instant::now(),
            entries,
        },
    );
    Ok(())
}

pub(super) fn list(
    app: &MacosContext,
    caller: String,
    computer_id: String,
    path: String,
    offset: usize,
    snapshot_id: Option<String>,
) -> Result<DirectoryPage, String> {
    crate::runtime::shutdown::ensure_accepting_operations()?;
    if caller.is_empty()
        || caller.len() > 128
        || caller.bytes().any(|byte| byte.is_ascii_control())
        || !valid_path(&path)
        || offset > MAX_ENTRIES
        || offset % PAGE_SIZE != 0
    {
        return Err("Invalid folder request.".into());
    }
    super::ensure_loaded(app)?;
    let (record, state) = super::computer(&computer_id)?;
    if state != super::store::State::Running {
        return Err("Start this computer to browse its files.".into());
    }
    if !record.setup.roots {
        return Err("Finish setting up this computer before browsing its files.".into());
    }
    let generation = super::computer_generation(&computer_id)?;
    if offset != 0 {
        return cached_page(
            &computer_id,
            generation,
            &caller,
            &path,
            offset,
            snapshot_id.as_deref(),
        );
    }
    let (layout, record) = super::layout_and_record(app, &computer_id)?;
    let output = guest_access::run_capped(
        &layout,
        &record,
        &guest_command(&path),
        None,
        Duration::from_secs(10),
        Some(MAX_LISTING_BYTES),
    )
    .map_err(|error| {
        if error == guest_access::OUTPUT_TOO_LARGE {
            TOO_LARGE.to_string()
        } else {
            error
        }
    })?;
    if output.status != 0 {
        return Err(FAILED.into());
    }
    let entries = parse_listing(&output.stdout, &path)?;
    let id = next_id();
    let result = page(&entries, 0, &id)?;
    insert_snapshot(&id, &computer_id, generation, &caller, &path, entries)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_workspace_paths_are_normalized_and_downloads_are_transfer_only() {
        assert!(valid_path(WORKSPACE));
        assert!(valid_path("/Users/silo/workspace/folder name/日本語"));
        for path in [
            "/",
            "/workspace",
            "/Users/silo/Downloads",
            "/Users/silo/workspace2",
            "/Users/silo/workspace/../etc",
            "/Users/silo/workspace//x",
            "/Users/silo/workspace/a/./b",
            "/Users/silo/workspace/a\n",
        ] {
            assert!(!valid_path(path), "{path:?}");
        }
        assert_eq!(transfer_roots(), [WORKSPACE, DOWNLOADS]);
    }

    #[test]
    fn guest_listing_parser_preserves_odd_names_and_never_opens_links() {
        let output = concat!(
            "ok\0d\0",
            "Zm9sZGVy\0",
            "l\0",
            "bGluaw==\0",
            "f\0",
            "YTtiCg==\0"
        );
        let entries = parse_listing(output, WORKSPACE).unwrap();
        assert_eq!(entries[0].name, "folder");
        assert_eq!(entries[0].kind, "folder");
        let named = |name: &str| entries.iter().find(|entry| entry.name == name).unwrap();
        assert_eq!(named("a;b\n").kind, "file");
        assert_eq!(named("link").kind, "symlink");
        assert!(GUEST_LIST.contains("if [ -L \"$entry\" ]; then"));
        assert!(GUEST_LIST.contains("physical=$(cd \"$root\" 2>/dev/null && pwd -P)"));
        assert!(GUEST_LIST.contains("if [ \"$physical\" != \"$expected\" ]; then"));
    }

    #[test]
    fn non_utf8_file_names_are_refused_instead_of_becoming_fake_paths() {
        assert!(parse_listing("ok\0f\0/w==\0", WORKSPACE).is_err());
    }

    #[test]
    fn pages_are_bound_to_the_caller_and_running_generation() {
        let token = format!("snapshot-test-{}", uuid::Uuid::new_v4());
        let computer = "c690f54f-4cc9-4950-8782-30f21e89d18b";
        insert_snapshot(
            &token,
            computer,
            7,
            "client-a",
            WORKSPACE,
            vec![Entry {
                name: "project".into(),
                path: format!("{WORKSPACE}/project"),
                kind: "folder".into(),
            }],
        )
        .unwrap();
        assert!(cached_page(computer, 7, "client-a", WORKSPACE, 0, Some(&token)).is_ok());
        assert!(cached_page(computer, 7, "client-b", WORKSPACE, 0, Some(&token)).is_err());
        assert!(cached_page(computer, 8, "client-a", WORKSPACE, 0, Some(&token)).is_err());
        snapshots()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&token);
    }

    #[test]
    fn listing_errors_and_malformed_records_do_not_look_empty() {
        for listing in [
            "",
            "ok",
            "ok\0d\0",
            "ok\0q\0eA==\0",
            "ok\0f\0Li4=\0",
            "missing\0",
            "denied\0",
        ] {
            assert!(parse_listing(listing, WORKSPACE).is_err(), "{listing:?}");
        }
        assert!(parse_listing("ok\0", WORKSPACE).unwrap().is_empty());
    }

    #[test]
    fn command_quotes_guest_paths_as_one_argument() {
        let command = guest_command("/Users/silo/workspace/a 'b'");
        assert!(command.starts_with("/bin/bash -c '"));
        assert!(command.contains("/Users/silo/workspace/a '\\''b'"));
    }
}
