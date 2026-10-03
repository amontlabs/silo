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
  wildcards (`cd`, `put`, `rename`, `ls` of the folder or of a wildcard-free name); local
  sources are staged behind a fixed symlink name. A file whose name contains `* ? [ ] { } \` cannot be downloaded, because
  `get` would expand it; uploading such names works.
- Conflicts are read from `ls -lan` after a `cd`. sftp ignores the status of a bare
  `ls`, so a listing only counts when it names the folder itself (`.`) and fits in the
  4 MiB output cap; an unreadable or oversized folder fails the transfer instead of
  looking empty. The first upload request uses the
  `ask` policy and sends nothing when a name exists; the Silo dialog then repeats it
  with Replace or Keep both (`name (1).ext`, default). Names repeated inside one batch
  are numbered too. Replace refuses to overwrite a folder. The viewer drop always keeps
  both.
- Uploads write `.<name>.silo-part-<id>` (non-wildcard characters only) beside the
  target and publish it. Replace uses the replacing `rename`. Ask and Keep both use
  `rename -l`, the legacy rename that fails when the target exists, so a file created
  after the folder was inspected is never overwritten. A plain `ls -lan` of the folder
  follows it and is not error-suppressed: the partial still listed means the name was
  taken, so the partial takes the next free `name (n)` (up to 8 tries, then an error);
  the target listed without the partial means it was published; a listing that fails,
  or shows neither name, fails the upload and removes the partial. The outcome lists the
  names actually stored. Failed or cancelled uploads remove the partial in a second short
  session. Keep-both numbering budgets the whole name including the extension within
  255 bytes. Downloads write a hidden partial beside the chosen destination, check its
  size, set ordinary permissions (0644, not the computer's) and rename it; the
  destination comes from the backend save dialog.
- A download is bounded on this device, because the client reads until the computer
  stops sending. The client runs with a file-size limit of the listed size plus 1 MiB
  (the operating system stops it), the partial is also watched every 50 ms and the
  transfer stopped as soon as it exceeds the listed size, and the destination volume
  must have the file plus 64 MiB free before it starts and at least 32 MiB while it
  runs. Every abort removes the partial and says why.
- The computer can change a file or folder between Silo checking it and the client
  reading it. The same session lists the file again just before `get`, and a result
  that is no longer the same regular file (or the same folder) is discarded. A swap in
  the remaining milliseconds can only change which of the computer's own bytes are read,
  within the bound above; nothing about it affects where or how the file is written on
  this device. Uploads cannot be hurt this way beyond what the computer could already do
  to its own files.
- Free space is read with `df` when the guest's SFTP server supports it; otherwise the
  check is skipped and a full disk is reported as a failed transfer.
- Limits: 4 GiB per file, 8 GiB and 100 files per upload, one transfer at a time.
  Throughput through the ProxyCommands has not been measured (plan item P8).
- Progress arrives as `silo://transfer-progress` events. `sftp` prints no progress in
  batch mode, so a download reports the size of its local partial and an upload asks the
  computer for its partial's size every 1.5 s through a second short session. Cancel
  ends the whole `sftp` process group (including the ProxyCommand) and removes the
  partial. Each `sftp` runs under the same parent-lifetime watchdog as the SSH forwards
  (`owned_tunnel`), so it also ends if Silo crashes or is force-quit, and a graceful quit
  cancels the running transfer and waits up to 5 seconds for it to clean up, before it closes SSH connections or stops computers; a transfer is admitted or refused under the registry lock, so one that races Quit is either refused or cancelled. A computer
  partial left by a force-quit stays hidden in the folder.
- The picker, dropped files and save dialog are native. The backend keeps the chosen
  paths and gives the window an opaque one-time token (valid 10 minutes, at most 16 held,
  bound to the window that received it); `upload_files` accepts only such a token, never a
  path, and it survives only an upload that asks about a conflict. The frontend never
  reads file contents or paths, and the guest page never receives them. Only the main
  window can pick and download; the main window and desktop viewer shells can upload
  (viewer shells only to Downloads), and the status panel can do neither. The
  `desktop-transfer` capability must be listed in `tauri.conf.json`
  (`app.security.capabilities`) to apply; `transfer-permissions.test.ts` checks the
  capabilities as configured.
- Transfers, their progress notification and the replace-or-keep dialog belong to the
  application, not to the Files page, so they continue and stay visible while the user
  moves between pages.

### Needs live verification

- Native drops over the computer's display (plan item P9). The child webview that shows
  the computer is a separate webview, so a drop on it is not a window event of the shell.
  `desktop_viewer.rs` registers `Webview::on_webview_event` on the child and
  `WindowEvent::DragDrop` on the shell; both call `transfer::native_drop`, which emits
  `silo://viewer-drag` and `silo://viewer-drop` (a token and names) to the shell window
  only; the shell listens on its own window, so another open viewer never hears them. Tauri's drag-drop handler consumes the drop, so the page gets no HTML5 drop event
  and no file contents. Confirm on macOS and Linux that a drop over the display uploads
  and that the page's `drop` listener never fires.
- `sftp` behavior against a real guest (`posix-rename` for Replace, `rename -l` against a
  Linux `sftp-server`, `df`, home folder creation) and over a remote computer; automated tests use OpenSSH's `sftp-server`
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
