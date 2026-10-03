# SiloUI Files

Implemented 2026-09-09. Files browses `/workspace` through the bundled runtime.
The existing repository panel, computer filters, pane layout and folder rows remain.
Single-file upload and download were added 2026-10-03 ([below](#upload-and-download)).
No content editor, deletion, folder transfer, recursive search or automatic boot
is included.

## Loading and refresh

- One directory per request, folders first, stable filename order, 200 entries/page.
- Native directory snapshots carry an ID. Further pages must match that ID; an
  expired/replaced snapshot requires a fresh listing instead of mixing scans.
- The frontend deduplicates requests and allows three concurrent loads. Cached
  folders render immediately; refresh preserves existing rows until all previously
  loaded pages succeed. First loads and pagination use skeleton rows. Failures
  show compact safe messages and Retry; pagination failures preserve current rows.
- Expanded folders refresh every ten seconds while Files is visible, and on window
  focus or visibility restoration. Collapsed/hidden folders do not poll. Computer state
  or freshness changes invalidate cached data and discard obsolete responses.
- The frontend retains at most 128 inactive directory records. Native listings have
  a 20,000-entry, 1 MiB output and 2 MiB estimated allocation limit. Its cache holds
  at most 64 snapshots, expires them after 120 seconds and stays around 8 MiB.
  Exceeding limits produces a compact failure, not a false empty/complete listing.

## Native boundary

`list_computer_directory(computer, path, offset, snapshotId)` checks managed computer
ownership and running state. Paths are validated and passed as positional arguments,
never interpolated into shell source. GNU find lists one level with NUL-separated
records; names containing spaces, Unicode or newlines retain their identity.
The guest script checks physical directory paths and lists links without following
them. This is not a security boundary against a malicious guest concurrently moving
its directories. Non-UTF8 filenames fail safely rather than becoming lossy paths.

The existing `msb exec` can automatically start stopped computers. A small bundled patch
adds `--no-start`, using the SDK's connect-only path. Silo bypasses its own temporary
boot wrapper for this mode and releases the GitHub update lock before reading.
Guest execution has a five-second timeout, with an eight-second outer limit; the
initial status inspection has the existing runtime read timeout.

Relevant pinned upstream source:
- [CLI execution](https://github.com/superradcompany/microsandbox/blob/5eca4de8bf233e57f114140f8c076ea8c96f21ab/crates/cli/lib/commands/exec.rs)
- [Rust sandbox implementation](https://github.com/superradcompany/microsandbox/tree/5eca4de8bf233e57f114140f8c076ea8c96f21ab/sdk/rust/lib/sandbox)

## Upload and download

Files uploads into the shown folder (**Upload files here** on the computer's root and
on every folder row) and downloads from file rows (**Download**). Both appear only on
the Files page; the status panel's folder picker has no transfer actions. Dropping
files on the desktop viewer uploads them to the computer's Downloads folder (see
[the viewer integration plan](SiloUI-VIEWER-INTEGRATION-PLAN.md#phase-4-file-upload-and-download)).

### Mechanism

`src-tauri/src/transfer.rs` runs the system OpenSSH `sftp` client (`/usr/bin/sftp`) in
batch mode (`-b`) with the managed SSH configuration and ProxyCommand the editor and
Git transport already use: `msb ssh serve --stdio --no-start` for a local computer,
`silo --remote-guest` for a computer on another device. A stopped computer is never
started ("Start this computer to transfer files."). The session runs as the working
account, so uploaded files belong to `silo`. The shared function
`editor::private_computer_transport` returns the alias and configuration for either
kind of computer.

Rejected, per the reuse policy in `AGENTS.md`:

| Alternative | Why not |
| --- | --- |
| `msb copy` | Starts stopped computers, writes root-owned files, and has no progress API. |
| `exec` with stdin | Needs a hand-written framing and integrity protocol. |
| JSON frames over `silo-remote` | Frames are capped at 4 MiB and remote computers would need a second path. |
| Selkies' own file transfer | Gives the guest page a channel to the device; it stays `none`. |
| Reimplementing SFTP | `sftp` is maintained, ships with both supported hosts and already handles the protocol. |

### Rules

- Roots: `/workspace` and the working account's `~/Downloads`. The requested path is
  checked lexically with `valid_path`, then the computer reports the folder's real
  location (`cd` then `pwd`), which must also lie inside a root; a link that leaves the
  roots is refused. Downloads additionally require the entry to be a regular file;
  links and folders are refused.
- Host names are sanitized (no `/`, NUL or control characters, not `.` or `..`, at most
  255 bytes). Remote names reach `sftp` only through commands that do not expand
  wildcards (`cd`, `put`, `rename`, bare `ls`); local sources are staged behind a fixed
  symlink name. A file whose name contains `* ? [ ] { } \` cannot be downloaded, because
  `get` would expand it; uploading such names works.
- Conflicts are read from `ls -lan` after a `cd`. The first upload request uses the
  `ask` policy and sends nothing when a name exists; the Silo dialog then repeats it
  with Replace or Keep both (`name (1).ext`, default). Names repeated inside one batch
  are numbered too. Replace refuses to overwrite a folder. The viewer drop always keeps
  both.
- Uploads write `.<name>.silo-part-<id>` (non-wildcard characters only) beside the
  target and `rename` it into place; failed or cancelled uploads remove the partial
  in a second short session. Downloads write a hidden partial beside the chosen
  destination, check its size, and rename it; the destination comes from the backend
  save dialog.
- Free space is read with `df` when the guest's SFTP server supports it; otherwise the
  check is skipped and a full disk is reported as a failed transfer.
- Limits: 4 GiB per file, 8 GiB and 100 files per upload, one transfer at a time.
  Throughput through the ProxyCommands has not been measured (plan item P8).
- Progress arrives as `silo://transfer-progress` events. `sftp` prints no progress in
  batch mode, so a download reports the size of its local partial and an upload asks the
  computer for its partial's size every 1.5 s through a second short session. Cancel
  ends the whole `sftp` process group (including the ProxyCommand) and removes the
  partial.
- The picker, dropped paths and save dialog are native; the frontend passes host paths
  back to the backend but never reads file contents, and the guest page never receives
  them. Only the main window can pick and download; the main window and desktop viewer
  shells can upload (viewer shells only to Downloads), and the status panel can do
  neither.

### Needs live verification

- Whether Tauri delivers drag-and-drop events for the window that hosts the guest child
  webview on macOS and Linux (plan item P9). The shell window listens with
  `onDragDropEvent`; if the child webview swallows drops, the drop target must move to
  the shell window over the viewer area.
- `sftp` behavior against a real guest (`posix-rename` for Replace, `df`, home folder
  creation) and over a remote computer; automated tests use OpenSSH's `sftp-server`
  on the test machine and scripted stand-ins.
- Throughput (P8) and the cost of the progress probe on large uploads.

## Verification

- Full frontend suite: 532 tests across 56 files passed; typechecking and lint pass.
- Five native Files tests and eleven runtime packaging tests passed.
- Live isolated macOS VM: 410 entries, Unicode/newlines, links, empty/missing folders,
  permission denial and stopped-VM refusal passed. Directory command execution was
  about 12 ms, excluding Silo's initial status inspection. The disposable VM and
  its isolated storage were removed; the user's dev VM was unchanged.
- Browser checks used only the existing fixture adapter through a temporary ignored
  harness, removed afterward. Wide and narrow Files layout, expansion and stopped
  feedback passed. Production has no fixture fallback or temporary preview entry.
- Linux hardware testing remains. This evidence does not claim instant cold loading
  on every filesystem or host.

## Test in Silo

1. Start a computer normally and open Computers > Files. Expand folders under its name.
2. Collapse and reopen a folder: cached rows should appear immediately.
3. Create a file using your terminal in that computer's `/workspace`. Return to Silo;
   the expanded folder refreshes on focus, or within ten seconds while visible.
4. Use a folder with more than 200 entries: Load more appends rows with skeletons
   during loading. Collapse folders or switch tabs to stop their background refresh.
5. Stop the computer normally and open Files: it explains that the computer needs starting.
   Browsing alone must never start it.

Final native verification: 223 ordinary tests passed, with five opt-in live tests
left ignored. The local HTTP-listener and filesystem-alias regressions passed in
separate permission-enabled runs. Final app checks caught and corrected the old
preflight patch pin; the checked-in-patch/hash regression now passes. Two existing
onboarding tests were updated to exercise supported CPU edits instead of renaming.

Final normal macOS bundle was rebuilt and reopened at
`app/SiloUI/src-tauri/target/debug/bundle/macos/Silo.app`. The runtime warning was
absent. Computers > Files showed `dev` with “Start this computer to browse its files.”
The VM remained stopped, and Silo was left open on Files for user testing.

The directory command is declared in [`src-tauri/build.rs`](../app/SiloUI/src-tauri/build.rs)
and granted to the main window in [`capabilities/preview.json`](../app/SiloUI/src-tauri/capabilities/preview.json)
and to the status picker in [`capabilities/status.json`](../app/SiloUI/src-tauri/capabilities/status.json).
Rust handler registration alone does not grant access. See
[Tauri capabilities](https://v2.tauri.app/security/capabilities/).
The [native permission regression](../app/SiloUI/src/test/native-permissions.test.ts)
checks registration, manifest entries and capability grants, including a snapshot
of non-main grants. Verify a rebuilt Dev app against a disposable running computer
separately; mocked invokes cannot prove native access.
