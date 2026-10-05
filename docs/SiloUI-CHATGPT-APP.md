# Pinned ChatGPT app on each device

Implements section 4 of the [computer use plan](SiloUI-COMPUTER-USE-PLAN.md).
Code: `app/SiloUI/src-tauri/src/chatgpt_app.rs` (this folder) and
`chatgpt_app/auto.rs` (the background worker), `computer_use.rs` (computer integration);
lock: `app/SiloUI/src-tauri/guest/chatgpt-app-lock.json`. Every device, remote
ones included, prepares its own copy. [Linux desktop](SiloUI-DESKTOP.md#built-in-computer-use)
describes what a computer does with the folder.

## Behavior

The LCU computer-use runtime needs the official ChatGPT Linux app. Silo never
publishes OpenAI files. There is no consent step (owner decision 2026-10-02):
every device running Silo downloads the pinned `.deb` from OpenAI by itself, in
the background, and keeps one read-only copy that all its computers mount. The only
disclosure is one sentence in the bundled help and in Settings, Connections, which shows it only in the "Computer use components" section that appears when a device's download failed or its status cannot be read: "Silo downloads
ChatGPT for Linux from OpenAI so agents in your computers can use the Linux
desktop." Guest architecture equals host architecture, so
the Debian architecture is `arm64` on Apple Silicon and Arm Linux, `amd64` on
x86-64.

`ensure(root, lock, arch, downloader, report)`:

1. Returns the published folder at once if it is *verified* (below; no lock,
   no network).
2. Takes an exclusive `flock` on `<root>/.lock`; concurrent callers (threads or
   processes) wait, then verify the folder again.
3. Removes whatever is under the published name that failed verification (a
   folder without a valid record, a damaged or tampered tree, a symlink): the
   record first, then the folder, which is moved aside to `.rejected-*` and
   deleted. Nothing is ever trusted because of its name.
4. Downloads the exact pool URL over HTTPS only (redirects must stay HTTPS)
   into `downloads/chatgpt_<version>_<arch>.deb.part`, resuming with `Range`
   after a failure, up to 5 attempts with 2/4/8/16 s backoff, 20 s connect
   timeout and a 30 minute bound per attempt (the blocking `reqwest` client has
   no stall timeout; a retry resumes where it stopped).
5. Verifies size, then SHA-256, against the lock. A failing file is deleted. A
   hash mismatch is not retryable.
6. Streams the decompressed `data.tar` from an established tool, validates
   every entry and writes the `usr/lib/chatgpt` entries into `.staging-*` on the
   same volume. No maintainer script ever runs. The first rejected entry aborts
   at once and kills the unpacking tools without draining the package.
7. Publishes in a crash-safe order: every file is synced as written; directories
   are synced bottom-up; the tree digests are computed from what is on disk;
   the staging directory is renamed into `published/<version>-<debarch>` and
   both parents are synced; the **publication record** (next to `published/`,
   not in it) is then written (exclusive temporary, sync, rename, parent sync). The record is the last durable step, so a crash
   at any earlier point leaves a folder that is never reused. The `.deb` is
   deleted after the record is durable. Every sync error aborts publication
   (and removes a tree whose record could not be written).
8. Returns the canonicalized absolute path (MicroSandbox refuses mount roots
   through symlinks, for example macOS `/tmp`).

A published folder is never modified. If the rename fails, a folder that
appeared meanwhile is accepted only if it passes verification.
Stale `.staging-*`, `.rejected-*` and temporary files are removed on the next
call.

`collect_garbage(root, lock, arch, in_use)` removes `published/<version>-<arch>`
folders (and their records) that are neither pinned nor in `in_use` (folder
names), stale staging directories and downloads of other versions. The app runs
it at start and after an update is prepared, and only while no computer runs: all computers
mount the whole `published/` folder and a running guest may still use the
previous version until its next boot sync.

`ensure_published_dir(root)` creates the root and an empty `published/` (mode
0755, so the guest's working account can enter the mount), moves a tree an
earlier build published directly under the root into `published/` (it is
verified like any other before use) and returns the canonical path computers mount.
The app calls it at start, so the folder exists before the download finishes.

## Filesystem safety

Another process, a previous run or a hostile computer's shared folder could leave links or
folders in the storage directory, so nothing is trusted by path:

- The storage root must be a real directory (never a symlink) owned by the
  current user; it is opened with `O_NOFOLLOW|O_DIRECTORY`, checked with
  `fstat` and, when Silo creates it, tightened to 0700. A root writable by
  others, owned by someone else or reached through a symlink refuses every
  operation (lock, status, ensure, garbage collection). Ancestors of
  the root (for example `~/Library`) are the user's own and are not checked.
- Subdirectories (`downloads`, `.staging-*`) are opened relative to that handle
  with `O_NOFOLLOW` and checked for ownership. All writes use `openat`-style
  calls (`mkdirat`, `openat` with `O_CREAT|O_EXCL|O_NOFOLLOW`, `symlinkat`,
  `renameat`, `unlinkat`) relative to those handles, one component at a time,
  so no component can be swapped for a link between a check and its use.
  Extraction opens each parent directory component by component the same way.
- Files Silo creates (record, download `.part`, tree files) are made
  exclusively. A planted `.part` (a symlink, a hard link, someone else's file)
  is deleted, never opened through. A resumed download re-checks what it opened
  (regular file, one link, owned by the user).
- Removal never follows links (`unlinkat`; directories are renamed aside and
  deleted with `remove_dir_all`).

## Publication record and verification

`<root>/<version>-<debarch>.published.json` sits next to `published/` (never
inside it, so the mounted folder holds only verified trees) and holds: schema version, lock version, architecture, the `.deb` SHA-256
from the lock, `treeSha256`, `statSha256`, entry count and byte count.

`treeSha256` is SHA-256 over the sorted list of (path, type, mode, size,
symlink target) plus each regular file's SHA-256. `statSha256` covers the same
shape and metadata plus file mtimes, without reading file contents. Trees with
a set-id or group/other-writable entry, a hard link, a device or anything that
is not a file, directory or symlink never have a digest.

A folder is "ready" (`verify_published`) only when:

1. Every call: the record exists, is a plain file owned by the user and parses;
   it matches the current lock (version, architecture, `.deb` hash); the root
   and the version folder are real directories owned by the user, not group or
   other writable; `ChatGPT`, `resources/cua_node/bin/node` and
   `resources/cua_node/bin/node_repl` are regular files with an execute bit; and
   the tree's stat digest equals the record (a few thousand `lstat`s, tens of
   milliseconds).
2. Once per process for each tree (and again whenever the stat digest or the
   identity of the record or tree changes): the full content digest equals
   `treeSha256`. Publication seeds the per-process cache, because it just
   computed the digest. The cache is in memory, so every app start verifies
   every byte once.

Trade-off: a same-size, same-mtime in-place edit made after the first check in
a running app is not seen until the next start. Anything that changes size,
mtime, mode, names, links or shape is caught on the next call. An
attacker who can edit the tree can also edit the record and the cache is not
a defense against that; the record binds the tree to the lock and detects
accidents, partial writes, stale or hand-made folders and tampering by
processes that do not also rewrite the record. Mounting is read-only, so a computer
cannot change it. Verification runs on whichever thread calls `ensure` or
`current_status`; the first full check of a process takes a few seconds in a
release build (see Verification), so UI code should not call it on the render
path.

## Paths (per channel)

Under `app_data_dir()/chatgpt`, which Tauri derives from the bundle identifier,
so production (`org.silo.preview`) and Dev (`org.silo.dev`) never share it, as
[build channels](SiloUI-BUILD-CHANNELS.md) require. On macOS that is
`~/Library/Application Support/<identifier>/chatgpt/`.

```text
.lock  downloads/  .staging-*/
<version>-<debarch>.published.json
published/<version>-<debarch>/
```

Each `published/<version>-<debarch>` folder holds what dpkg would place in
`/usr/lib/chatgpt`: `ChatGPT`, `resources/…`. `published/` is what computers mount
read-only at `/opt/silo/chatgpt`; it holds only verified trees and is
garbage collected, which keeps MicroSandbox's first `statfs` walk of the mount
small (#1701/#1702). Records, staging and downloads stay outside it. A `consent.json` left by an
earlier build is ignored.

## Automatic preparation

`chatgpt_app/auto.rs` runs one background worker per process (an in-process slot;
`ensure` also holds the cross-process lock, and `PREPARING` covers a second
caller in the process). `computer_use::install` starts it at app start on a
background thread: read the status (the first full digest of a process runs
here), collect unused versions, and, unless the pinned version is published, wait
10 s and start the worker at low priority (utility QoS on macOS, nice 10 on
Linux; the unpacking tools inherit it). The worker loops: run `ensure` (which
itself retries a broken connection 5 times with 2 to 16 s backoff and resumes the
`.part` file), then

- ready: sync running built-in computers at once and collect garbage; stop;
- retryable failure (network, firewall, disk space): wait 30 s, 1, 2, 5, 10, 30
  min, then hourly, and try again; the failure stays the reported status
  meanwhile (the clear "firewall or network filter may be holding Silo's
  connection" message is kept);
- failure retrying cannot fix (hash mismatch, OpenAI no longer serves the
  pinned version): stop; **Retry** starts a new worker.

Retry (`chatgpt_app_retry`, below) wakes a waiting worker, which restarts the
schedule, or starts a worker. Nothing waits for the download: computer creation, start
and restore only need the (possibly empty) `published/` folder. Silo does not
detect metered networks; an offline or filtered connection costs only the later
retries. The status survives as the in-process cache until the next attempt and
is recomputed from disk at every start.

## Status

`Status` serializes with a `state` tag: `idle` (waiting to download),
`downloading {receivedBytes,totalBytes}`, `verifying`, `extracting`,
`ready {path,version}`, `failed {reason,retryable}`. The reporter is called
from the worker thread (downloads throttled to 4 per second). The controller
adds `unknown` for a device whose status it cannot read.

### Commands

Both take an optional `device`: the id of a remote device (omitted: this
device). A computer target is rejected; there is no placeholder computer routing.

- `chatgpt_app_status { device? }` returns the status. It never blocks the UI
  thread and never re-verifies in the render path: reads use a cache filled at app
  start and by progress events; the first full digest of a process runs on a
  background thread at start. For a remote device it calls the bridge method
  `chatgpt.status`; an owner whose Silo lacks computer use answers `{"state":
  "unknown"}` (not an error), and the frontend maps any state it does not know,
  such as an older Silo's `notConsented`, to `unknown`.
- `chatgpt_app_retry { device? }` asks that device to try now (bridge method
  `chatgpt.retry`, a change, device level) and returns its status at once; the
  owner downloads. An older owner reports it unsupported.

`chatgpt-app-status` events carry this device's status for the UI (`device:
null`); a remote device has no events and is polled by the frontend (3 s while
it works, 15 s otherwise). Removed on 2026-10-02: `chatgpt_app_accept_notice`,
`chatgpt_app_prepare`, bridge methods `chatgpt.accept` and `chatgpt.prepare`,
`consent.json` and the `notConsented` state.

When preparing finishes with `ready`, running built-in computers on this device set
computer use up at once (see below); stopped ones do it when they start.

## Extraction and validation

Tool choice, per [reuse established tools](../AGENTS.md): macOS runs
`/usr/bin/tar -xOf x.deb data.tar.xz` into `/usr/bin/tar -cf - @-` (bsdtar
reads the `ar` container and rewrites the member as plain tar); Linux runs
`dpkg-deb --fsys-tarfile` (dpkg is essential on Ubuntu). Rust has no xz
decoder in the dependency graph, and adding `xz2`/`liblzma-sys` would put a C
library in the build for something the OS already provides. The existing `tar`
crate parses the stream; Silo writes files itself so no entry is trusted.

Per entry inside `usr/lib/chatgpt`:

- Only regular files, directories and symlinks. Hard links, devices, FIFOs
  and anything else abort. Entries outside the tree are skipped, never written.
- Names must be UTF-8, relative, free of `..`; a leading `./` is accepted.
- No setuid or setgid bit. Files become `0755` if any execute bit is set, else
  `0644`; directories `0755`.
- Symlink targets are relative; leading `..` may not exceed the link's depth
  and no `..` may follow a normal component (so a chain through another
  symlink cannot leave the tree). No entry may be written through a symlink.
- Case-insensitive collisions and duplicates abort; files are also created
  with `create_new`, so a name the filesystem folds (APFS) fails.
- Limits (checked with `u64` checked arithmetic): 200,000 entries; 8 GiB
  written in total, accounted on each entry's *effective* size (`Entry::size`,
  which honors a PAX `size` record that overrides the header) before any byte
  is written, and again on the bytes copied; entries outside the tree count
  towards the entry limit; paths and link targets up to 4096 bytes (this
  includes GNU long names) and 255 bytes per component; PAX extended header
  records up to 64 KiB per entry; directories and links must have size 0; the
  tar stream itself is capped at twice the byte limit, which bounds metadata
  the `tar` crate buffers internally.
- ChatGPT, `resources/cua_node/bin/node` and `resources/cua_node/bin/node_repl`
  must be regular, executable files (symlinks inside the tree are followed
  to check this).

The first violation aborts extraction immediately and discards the staging
folder; nothing is published.

## Sources

Version 26.928.31416 comes from OpenAI's apt repository
(`https://persistent.oaistatic.com/codex-app-prod/linux/deb`, suite `stable`,
key `3BFA0E4AE8B8CC16A2D9BA684A3B4A566C4660E4`), whose newest version is now
26.928.40906; older versions remain in the pool. Checks on 2026-10-01: the
pool URL for both architectures answers with the locked sizes (453,121,290 and
474,894,546 bytes); the arm64 SHA-256 was recomputed from a fresh download and
equals the lock. The amd64 hash comes from the signed index as recorded for the
plan and has not been recomputed locally; the OpenAI InRelease signature could
not be checked here (no gpg), only its hash chain to `Packages`.

The runtime pair is `0.0.27/20260927214556-b77d38801cca`
(`resources/cua_node/manifest.json`). `lcuVersion` is `0.9.4`, the LCU release
tested with it (the published v4 image still stages 0.8.1; see
[the computer use plan](SiloUI-COMPUTER-USE-PLAN.md#lcu-083-pin-2026-10-02)); `guest/lcu-lock.json` pins that release's archives (a test
checks that both locks agree). The owner updates the pair by hand: bump the
app lock and `lcu-lock.json` together.

## Verification

Unit tests (`cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml --locked
chatgpt_app`) build synthetic `.deb` files (ar container plus gzip data member)
in the test and cover hash and size mismatch, a first call with no stored choice, absolute, `..`,
escaping and chained symlinks, write-through-symlink, setuid/setgid, devices,
hard links, case collisions, duplicates, missing or non-executable required
files, atomic publish, reuse after interruption, concurrency, garbage
collection and status JSON. `chatgpt_app/auto.rs` tests cover the retry schedule, retries until ready, stopping on a failure retrying cannot fix, the early-wake restart, the single worker, the ready hook running once, and that no consent path remains in the source, `build.rs` or the capabilities. `tests/hardening.rs` adds planted symlinks (download
part file, staging folder, version folder, storage root, downloads folder), a preseeded fake version folder, a forged tree under a genuine
record, tampering after publication (same-size edit seen by the full digest,
size, added, removed, relinked and loosened-mode changes seen by the cheap
check), an interrupted publish (no record, torn record, partial tree), record
binding to the lock, PAX size larger than the header size, checked size sums,
long-name, PAX and entry-count limits, abort at the first rejected entry, and
exclusive file creation. They use temporary directories and no process-wide
Silo state (the in-memory verification cache is keyed by path).

Opt-in live test, downloads 453 MB and unpacks about 1.5 GB into a temporary
directory (set `SILO_LIVE_CHATGPT_ROOT` to keep the published folder):

```sh
SILO_LIVE_TEST_CONFIRM=disposable-test-fixtures \
  cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml --locked \
  chatgpt_app::tests::live -- --ignored --nocapture
```

Run on 2026-10-01 (macOS arm64, Rust 1.94.0): download, verification,
extraction and publication took about 52 s; the layout check and idempotent
second call passed.

Run on 2026-10-02 after the hardening (macOS arm64, Rust 1.94.0, release test
profile, real package: 4,479 entries, 1,560,896,612 bytes): download,
verification, extraction, sync, digest and publication took about 58 s; the
cheap reuse check (record, executables, stat digest) took 28 ms; the first
full content digest of a process took 6.2 s. In the unoptimized debug test
profile SHA-256 is about 20 times slower (the full digest took 121 s), which
is a test-profile artifact only.

## Integration (2026-10-02)

The mount, guest flow and commands are in `computer_use.rs`; the guest side in
`guest/silo-computer-use.py`; the image side in `guest-image/Dockerfile`.

- **Mount.** A computer created from a v4 or later image (`desktop.builtIn`) is
  created with `-v <canonical published dir>:/opt/silo/chatgpt:ro`; creation
  fails if Silo has no such folder rather than make a computer that can never get
  computer use. Every restore (import, transfer, checkpoint restore and fork)
  passes the same `-v` again, because MicroSandbox never carries host mounts in
  disk snapshots, and checks that the restored computer reports a read-only `Bind`
  mount at that path. Exports drop the mount from the saved configuration (its
  host path means nothing elsewhere); `desktop.builtIn` in the computer configuration is
  what makes the importing device mount its own folder. Pre-v4 computers never get
  the mount.
- **Guest.** Silo pushes the helper and the pinned pair into the guest and runs
  `apply` after every boot and when the app becomes ready, on a host background
  thread within a bound; see
  [Linux desktop](SiloUI-DESKTOP.md#built-in-computer-use).
- **Evidence** (2026-10-02, macOS arm64, MicroSandbox 0.7.6, real v4 image and
  real app): a fresh VM mounted the folder read-only (`ro` in `/proc/mounts`,
  writes fail with EROFS), `lcu status --json` reported `tested`, `lcu doctor`
  (which lists windows and takes a screenshot on Linux) passed within seconds of
  boot, `--approval auto` wrote `default_tools_approval_mode = "approve"` for
  Codex, and a restore with the mount passed again became ready again.

### Unpacking process ownership

The macOS tar pipeline owns both children. If its consumer cannot start, it kills
and waits for the producer before returning the spawn error. Dropping a Rust
[`Child`](https://doc.rust-lang.org/std/process/struct.Child.html) neither stops
nor reaps it. The synthetic `tar_pipeline_reaps_the_producer_when_the_consumer_cannot_start`
regression forces a missing consumer executable and verifies `waitpid` returns
`ECHILD` for the producer. It uses a disposable child, without packages or computers.
