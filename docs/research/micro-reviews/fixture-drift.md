# Frontend fixture drift

## Directory listings

The preview directory loader returned empty success for missing paths, listed
stopped sandboxes, resolved remote requests by a bare name, and paged mutable
arrays under a constant snapshot ID. The native
[directory command](../../../app/SiloUI/src-tauri/engine/src/files.rs) validates paths
and 200-entry offsets, checks running VM state, sorts folders before files,
and binds pagination to an owner/path snapshot for two minutes. The
[guest helper](../../../app/SiloUI/src-tauri/guest/list-directory.py) reports
missing and non-folder paths separately.

Fixture regressions failed before the correction and now exercise those native
states and errors. The real native stopped-state string, `Start this VM to browse
its files.`, also exposed a frontend allowlist mismatch. The directory store now
translates that known error to the existing sandbox terminology instead of
hiding it behind a generic failure. Checks use deterministic fixtures only;
no installed app or live VM is covered.

## Log pages

The static log-page helper accepted more than 200 records, had no response-byte
budget, omitted owner names/session, and used a remote UI target as the native
sandbox ID. It also treated source `all` as no matches, paginated context windows,
and accepted query failures that native rejects with structured bridge errors.
The native [log query](../../../app/SiloUI/src-tauri/engine/src/runtime_logs.rs),
`cached_page`, `read`, and `context` establish the 200-record/1 MiB response bounds,
the unfiltered 101-record context, and the query failure wording.

Regressions fail against the old helper and now cover those cases. History tests
use bounded native-sized responses to exercise retention, including cursor
cycles and inactive-cache eviction. The UI eviction/export regression uses long
records across several byte-limited pages instead of an impossible 3,000-record
response. The malformed-response regression remains intentionally malformed to
check frontend containment. Static log pages do not simulate native index
caching or file retention.

## Checkpoint outcomes

The preview fork action ignored its source and checkpoint, copied the first VM's
live runtime fields, and returned success for missing selections. Restore never
published a pending checkpoint reference and always fabricated a full recovery
point. Native [checkpoint commands](../../../app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs),
`fork_source`, `fork_commit`, `pending_workspace`, and `restore_steps`, resolve the
selected owner, publish a stopped VM with empty runtime fields, and retain
snapshot lineage and scope until Start. Recovery captures are full for running
VMs and disk for stopped VMs; a pending VM preserves its existing native member
under a new public recovery checkpoint ID.

The hook regressions cover local and remote ownership, missing-selection string
errors, duplicate names, current-state capture, pending-state forks, and recovery
scope/native-member references. These deterministic outcomes do not simulate
runtime checkpoint I/O, operation admission, or failure recovery journals.
Capture cases also exercise native label trimming/validation, newest-first
ordering, native-shaped generated IDs, and remote owner resolution.
