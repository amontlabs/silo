# SiloUI review remediation: design notes

Decision records for the items marked **design** in Phase 0 of the
[review remediation plan](SiloUI-REVIEW-REMEDIATION-PLAN.md). Owner decisions
come from [the 2026-09-29 review](research/codebase-review-2026-09-29.md).
Written 2026-09-29 against `main` at `edd30ae`. Every item follows
`AGENTS.md` "Reuse established tools".

**Evidence labels.** *Verified* means read in this repository or fetched from a
primary source on 2026-09-29. *Assumed* means taken from memory or an
inference that still needs a test. Where a source was read through a
summarising fetch rather than line by line, the note says so.

Sources checked:

- MicroSandbox pin: `app/SiloUI/runtime-inputs.json` (0.7.2, commit `60d4dc8`)
  and the single Silo patch `app/SiloUI/patches/microsandbox-silo-network-0.7.2.patch`,
  which does not touch snapshot or archive code (verified with grep).
- MicroSandbox v0.7.2 `docs/sandboxes/snapshots.mdx` and
  `sdk/rust/lib/backend/local/snapshot/archive.rs` (both read through a summarising fetch).
- tao `dev` `src/platform_impl/macos/app_delegate.rs` (summarising fetch);
  the locked version is tao 0.35.3 (`Cargo.lock`).
- Tauri single-instance plugin guide, <https://v2.tauri.app/plugin/single-instance/>,
  and plugin source `plugins-workspace/v2/plugins/single-instance`
  (`Cargo.toml`, `src/platform_impl/macos.rs`; summarising fetch).
- GitHub REST docs, "Create an installation access token for an app".
- `Cargo.lock`: `zbus` 5.19 and 4.4, `parking_lot` 0.12.5, and `thiserror` 2 are
  already in the dependency graph, so none of them adds a new supply-chain root.

---

## D-02: per-VM snapshot reads instead of the computer-wide idle check

**Problem.** `read_application_snapshot_once` rejects the whole read with
UPDATING whenever `OPERATIONS.is_idle()` is false. That includes hidden
housekeeping and work on unrelated VMs. As a result, one checkpoint or slow
boot freezes every row, and the first read at launch fails
(`runtime.rs:1748,1851`, `operation_gate.rs:579`). Related: H-37 and the F
update-readiness check.

**Options.**

1. Keep the global check and exclude hidden entries.
2. Settle each VM separately: read everything, then keep a VM's fresh fields
   only if `is_vm_idle(id)` holds and its generation is unchanged. Otherwise
   return that VM's previous value marked `settling`.
3. Use a versioned store with a revision per VM (for example `arc-swap`).

`is_vm_idle` and `generation(id)` already exist (`runtime.rs:1939` uses the
same pattern). Option 3 adds a subsystem this problem does not need.

**Decision.** Option 2, plus option 1 for computer-scoped fields. Hidden
entries never make a read stale. A computer-scoped visible operation marks only
the computer-level fields as settling.

**Outline.**

1. `operation_gate.rs`: add `is_idle_visible()` and a `snapshot_epoch(id)` that
   returns the generation together with the idle state.
2. `runtime.rs`:
   - Replace the `&|| OPERATIONS.is_idle()` predicate with a per-VM filter.
   - Remove the `Err(UPDATING)` return for partial overlap.
   - Add `settling: bool` per VM to the contract.
3. H (`production-source.ts`): never replace a fresher VM reading with a
   settling one (H-37).
4. `updates.rs:169`: use `is_idle_visible()`.

**Tests.**

- A running op on VM A leaves B's reading fresh.
- A hidden log cleanup never yields UPDATING.
- The launch read succeeds while `start_at_launch` runs.

**Open question.** Should settling rows show a spinner, or silently keep the
last value?

---

## E-03: checkpoint deletion with fork dependencies and native snapshot cleanup

**Problem.** Silo never removes native snapshots. Restore, Fork and failed
captures or imports accumulate full-size members. Deleted sandboxes leave
orphans, and the 1 MiB index eventually breaks export and import (E-03, E-23,
owner decision 9).

**What 0.7.2 supports** (verified in the v0.7.2 snapshot guide, read through a
summarising fetch):

- The subcommands are `create`, `list|ls`, `inspect`, `remove|rm`, `save`,
  `load`, `verify`, `head` and `reindex`.
- `remove` refuses the current group head while other members remain; the user
  must select another head first. `--force` does not bypass this guard.
- `remove` also refuses a snapshot that has indexed children. `--force` exists
  for that case, but its exact semantics are unverified.
- The guide says loaded snapshots and restored children own their required
  files, so removing a base does not break them.

Silo's patch does not change any of this.

**Options.**

1. Call `msb snapshot remove` per member. Silo keeps the dependency policy and
   MicroSandbox keeps storage integrity.
2. Delete snapshot directories directly and run `reindex`. This bypasses the
   upstream guards, so it is rejected.
3. Add a MicroSandbox prune/GC upstream. This is the long-term option, but no
   such command exists in 0.7.2.

**Decision.** Option 1. Never pass `--force`.

- A checkpoint is **pinned** when another Silo checkpoint or a pending
  restore/import journal references it. Pinned checkpoints show "Used by
  {fork}" and cannot be deleted.
- MicroSandbox's children guard is a second line of defence. Silo still
  reports the block in its own words.

**Outline.**

1. `runtime/checkpoints.rs`: add `delete_checkpoint(id, member)` under the VM
   gate:
   - Check pins and head.
   - Move the head with `snapshot head` first if needed.
   - Call `snapshot remove`, then drop the record.
2. `remove_deleted_snapshots`: journal the deleted sandbox's native members,
   then remove unneeded members in dependency order, moving each group's head
   when required. Keep fork dependencies journaled before `forget_removed`
   clears the sandbox's history. Run the same path after interrupted deletion.
3. On capture or import failure, remove the partial member or `silo-import-*`
   group (shared with E-23 and E-24 journaling).
4. No age-based sweep. Delete unneeded data at sandbox deletion, retaining exact
   native member identities in `checkpoint-cleanup.json` while forks or other
   dependents need them. Deleting the last dependent retries those identities.
   Failed or cancelled captures and imports remove their partial data immediately.
   At next launch, retry only data identified by an interrupted operation's own
   journal: checkpoint in-flight records, deletion candidates, and import/export
   journals. Never infer ownership or interruption from age.
5. UI: `checkpoint-panel.tsx` Delete action (decision 8, inline two-step);
   add checkpoint storage to the Storage tab.

**Tests.** Unit tests with the existing fake `msb` runner:

- pinned refusal
- head move before removal
- failed-capture cleanup
- deletion keeps a fork's base, then removes it when the last fork is deleted
- launch cleanup selects only journaled identities, preserving unrelated orphans
- failed deletion retries after launch without removing a reused member name

Also a `mac` live run for storage reclamation.

**Owner policy.** A fork's pinned base stays until its last dependent is
removed. "Before restore" and "Fork point" checkpoints have no age-based
collection policy; explicit checkpoint or sandbox deletion removes them.

**Implementation findings (2026-09-30).** Verified by reading the pinned
MicroSandbox source (`crates/cli/lib/commands/snapshot.rs`,
`sdk/rust/lib/backend/local/snapshot/{store,group,lineage,create,archive}.rs`):

- Each capture's parent is the sandbox's lineage cursor
  (`sandboxes/<name>/snapshot-lineage.json`), or the snapshot it was restored
  from. Successive checkpoints of one sandbox therefore form a parent chain, and
  the children guard refuses every checkpoint that has a later capture.
- `--force` only bypasses that children guard. Artifacts copy their own layers,
  so restores survive a removed parent, but `snapshot save --with-parents`
  (Silo's export) fails when any ancestor is missing. Never passing `--force`
  is therefore required, not only cautious.
- For the same reason Silo also keeps a sandbox's *lineage position* (its
  cursor, or the snapshot it was restored from): removing it would break that
  sandbox's next export. The newest checkpoint of a sandbox can be deleted only
  after it no longer builds on it (for example after a Restore to another one).
- `snapshot list --format json` reports `snapshot_id` and `parent_digest` (the
  parent's snapshot id); `size_bytes` is the disk's virtual size, so Silo
  measures artifact directories instead.

Implemented: Delete checkpoint refuses pinned checkpoints with the reason
(another record, a pending start, a later capture, a lineage position).
Sandbox deletion journals exact members and removes the ones no dependent needs;
deleting the last fork retries its pinned base. Launch retries only those deletion
journals and checkpoint captures still recorded as in flight. Export records its
capture before creation, removes an incomplete member on failure or relaunch, and
keeps a verified capture while a sandbox uses it as its lineage parent. Import
records its new group before load, removes indexed unfinished members even before
allocating a sandbox identity, and keeps ownership journaled when removal fails.
A failed, cancelled, timed-out, or killed load removes exactly the stages named by
that journal. There is no age-based startup sweep. Deleting checkpoints of a
sandbox on another computer is done in Silo on that computer.

**E-03 import crash gap closed.** The bundled
[operation-stage patch](../app/SiloUI/patches/microsandbox-import-stage-id-0.7.14.patch)
extends the pinned
[archive loader](https://github.com/superradcompany/microsandbox/blob/60d4dc8a436fb9365491567ec21d073e924e3c6d/sdk/rust/lib/backend/local/snapshot/archive/batch.rs)
with `snapshot load --stage-id <32 lowercase hex digits>` and Rust
`LoadOpts.stage_id`. Before opening an archive, the loader exclusively creates
`snapshots/.msb-snapshot-load-<id>` and `cache/tmp/snapshot-load-<id>` under the
configured runtime home. Archive unpacking, external-base unpacking, and batch
publication all stay inside these two roots. Separate roots preserve the
snapshot/cache filesystem placement required by rename-based publication.
Existing roots, including symlinks, are refused, never adopted. Normal return or
failure drops the temporary-directory guards; process death leaves deterministic
paths. The patch adds no database, scheduler, or age/prefix collection policy;
it extends the existing Apache-2.0 loader and tempfile ownership mechanism.

Silo chooses `silo-import-<id>`, checks that both the group and stage paths are
absent, then fsync-saves the group in its existing operation journal before
passing its suffix as `--stage-id`. This saved group is also the durable stage
identity; a second independently mutable field would permit mismatches. Cleanup
validates the group, removes only the two derived stage paths, then removes that
group's indexed members without `--force`. Missing stages are harmless, including
a crash between saving the journal and spawning load. The existing inherited snapshot-worker lock prevents removal while a child
from the previous Silo process is still writing. Cleanup errors preserve
the journal for the next launch. Symlinked stage paths or parent components fail
closed. Older random stages and stages from other operations stay untouched.

The patch fixes a reusable upstream API gap. Upstream source creates random
per-archive snapshot/cache paths and random publication paths; recording only the
chosen snapshot group cannot identify them before publication. The smallest fix
is caller-owned deterministic stage roots, rather than a new Silo staging
subsystem or sweeping names. Runtime patch pins and capability checks must move
together. See [runtime packaging](SiloUI-RUNTIME-PACKAGING.md) for fixture-only
build and killed-load evidence; this proof does not qualify live VMs or packages.

---

## F-02 + F-20: graceful quit on macOS Dock/logout and on Linux logout/shutdown

**Problem.**

- macOS: Dock Quit, logout and any Apple-event quit call `terminate:`. tao has
  no `applicationShouldTerminate:`, so `ExitRequested`, `stop_local_vms` and
  the frontend flush are skipped (F-02).
- Linux: there is no SIGTERM handler and no logind inhibitor (F-20).
- Owner decision 7: confirm on user Quit; never prompt on logout or shutdown,
  but still stop gracefully.

**Upstream status.** The `app_delegate.rs` summary lists `applicationDidFinishLaunching:`,
`applicationWillTerminate:`, `application:openURLs:`, the user-activity pair,
`applicationShouldHandleReopen:hasVisibleWindows:` and
`applicationSupportsSecureRestorableState:`. It does **not** list
`applicationShouldTerminate:` (tao `dev`). An unauthenticated GitHub PR search
for the selector in tao returned no results. That search may be incomplete.

Tauri tracks the gap in these issues (from a GitHub issue search):

- tauri-apps/tauri#9198: "`ExitRequested` not fired on macOS" (open)
- #12978: a feature request for `applicationShouldTerminate(_:)`, closed as a
  duplicate
- #14558: OS-shutdown graceful-exit docs (open)

Upstream has not shipped a fix.

**Options (macOS).**

1. Fork or vendor tao. This is heavy, and tao is pulled transitively through
   tauri-runtime-wry.
2. Local runtime patch: in `setup`, add `applicationShouldTerminate:` to the
   class of `NSApp.delegate()` with `objc2::ffi::class_addMethod`. The delegate
   returns `NSTerminateLater` and routes into the F-03 quit path, which later
   calls `replyToApplicationShouldTerminate:`. Silo already depends on `objc2`.
3. Upstream a tao PR that maps the selector to an `ExitRequested`-style event
   with a deferred reply.

**Decision.** Implement option 2 now and file option 3 in parallel. Remove the
local method once a tao release that Tauri uses carries it.

- Distinguish logout or shutdown from a user Quit through the current Apple
  event's `kAEQuitReason` (`kAELogOut`, `kAEReallyLogOut`, `kAEShowRestartDialog`,
  `kAEShutDown`, …). This is *assumed* from Apple's API; verify it live.
- Logout path: no prompt, bounded `stop_local_vms` (≤ 20 s), then reply YES.
  Reply NO only if the user cancels a Quit.
- Fallback: a bounded `stop_local_vms` on `RunEvent::Exit`.

**Options (Linux).**

- logind `Inhibit("shutdown:sleep", "Silo", "Stopping sandboxes", "delay")`
  over `zbus` (already locked), listening for `PrepareForShutdown(true)`. The
  delay is capped by logind `InhibitDelayMaxSec`, 5 s by default (assumed from
  logind docs).
- SIGTERM via `tokio::signal::unix` routed into the same graceful exit, since
  session logout sends SIGTERM to the app scope.

**Decision.** Use both.

- Take the delay lock at startup and release it after `stop_local_vms`.
- Stop VMs in parallel so the stop fits the 5 s cap.
- Beyond the cap, rely on msb's own VM teardown. This is assumed and needs a
  `linux` test.

**Outline.**

1. New `src-tauri/src/system_shutdown/{macos,linux}.rs` with a
   `ShutdownReason { UserQuit, Logout, Shutdown }` into the F-03 quit request.
2. Unit-test the quit-reason routing.
3. Live checks: `mac` Dock Quit, logout cancel/allow and `osascript quit`;
   `linux` GNOME logout and `systemctl reboot`.

**Open questions.**

- Should Silo raise `InhibitDelayMaxSec` through a packaged logind drop-in
  (Debian package only)?
- Is losing VM state acceptable if logout exceeds the cap?

**Implementation record (F-02, F-20).**

- macOS: `system_shutdown/macos.rs` adds `applicationShouldTerminate:`
  (encoding `Q@:@`) to the class of `NSApp.delegate()` with
  `objc2::ffi::class_addMethod`, between `Builder::build` and `App::run`, then
  assigns the delegate again so AppKit re-reads its optional methods. If a
  future tao already implements the selector, `class_addMethod` fails and Silo
  keeps tao's method. The handler replies `NSTerminateLater`; the Quit path
  answers with `replyToApplicationShouldTerminate:` (YES after VMs stop and
  settings save, NO when the user cancels or Quit fails). No new crate: `objc2`,
  `objc2-foundation` and `objc2-app-kit` were already dependencies. The
  `kAEQuitReason` descriptor is read with `msg_send!`, because the typed
  accessor needs `objc2-core-services`, which is not in the graph.
- Linux: `zbus` 5 (already locked through `ksni` and the single-instance
  plugin) takes a logind `shutdown` delay lock only. Suspend is not inhibited:
  a `sleep` delay lock without handling `PrepareForSleep` would delay every
  suspend and sleeping must not stop sandboxes. The stop budget is
  `InhibitDelayMaxUSec` minus 750 ms (at least 1 s, at most 20 s).
- SIGTERM (Linux logout, `systemctl stop`, `kill`) is handled on both
  platforms with Tokio's `signal` feature (Tokio was already a dependency) and
  enters the same no-prompt path with a 20 s budget.
- Session end never cancels the exit: a failed or late stop is logged and Silo
  exits. `RunEvent::Exit` without an approved Quit (and not an update restart)
  runs a bounded stop as a backstop.
- Shutdown dispatches local VM stops concurrently under the computer gate.
  The parent shares one worker flock with scoped stop workers so child stops
  overlap without admitting another process's runtime mutation. Slow enumeration
  or inspection and individual stop latency can still exceed logind's default
  5 s delay; live logout verification remains required.

---

## F-09: single instance

**Problem.** A second launch runs `runtime_migration::install`, fails
`remote::start` (C-05) and panics at `.expect("failed to build Silo")`. On
Linux, clicking the launcher while Silo is hidden in the tray does nothing
(`main.rs:214,252-253`).

**Options.**

1. `tauri-plugin-single-instance`: the official plugin.
   - Verified in the plugin guide: it supports Windows, Linux and macOS.
   - On Linux it uses D-Bus (`org.{id}.SingleInstance`).
   - It "must be the first one to be registered".
   - The callback receives `(app, argv, cwd)`.
   - Snap and Flatpak need manifest D-Bus permissions. Silo ships deb and
     AppImage only.
2. A custom lock file or socket in `~/.silo`. Rejected under `AGENTS.md`.

**Decision.** Option 1.

- Register it first on the builder.
- The callback shows, unminimizes and focuses the main window (tray-hidden case
  included).
- Move all migration and `remote::start` work so it runs only after the plugin
  has decided this is the primary instance, inside `setup` after plugin init.
  Nothing mutating runs before `Builder::build`.

**Outline.**

1. `Cargo.toml`: add `tauri-plugin-single-instance = "2"`.
2. `main.rs`:
   - Reorder startup.
   - Replace `.expect` with a logged error dialog.
3. Linux: confirm that the D-Bus session bus exists under AppImage.

**Tests.** Unit-test the startup ordering (migration not called before primary
check). Run `linux` and `mac` double launches.

**Open questions.**

- Debug and release builds use separate identifiers, so both can run at once
  and share `~/.silo`. Should they share one lock? The plugin keys by
  identifier (assumed).
- On macOS the plugin listens on `/tmp/{identifier}_si.sock`. This was
  verified in the plugin source through a summarising fetch.
  - The directory is shared and world-writable.
  - If the socket path is stale or refuses connections, the plugin removes it
    and claims the singleton.
  - Another local user who binds that path first could make Silo exit at
    launch and receive its argv and cwd.
  - Given the single-user Mac target, is this acceptable, or should the macOS
    side be upstreamed or patched to use `$TMPDIR`, which is per user?
    LaunchServices already de-duplicates `open` launches on macOS.

**Implementation record (F-09).**

- Pinned `tauri-plugin-single-instance = "~2.4.5"` (Apache-2.0 OR MIT,
  maintained in `tauri-apps/plugins-workspace`). 2.5.x requires Tauri 2.12;
  Silo pins Tauri 2.11. 2.4.3 moved the macOS listener to a Tokio socket, so
  2.4.5 is the oldest acceptable release. Its dependencies (`zbus` 5, `tokio`,
  `tracing`, `thiserror` 2) were already in `Cargo.lock`.
- Verified in the 2.4.5 source: macOS uses `/tmp/{identifier}_si.sock` and
  exits the second process with `std::process::exit(0)` from the plugin's
  setup; Linux owns `{identifier}.SingleInstance` on the session bus and exits
  the same way. Plugins initialize inside `Builder::build`, before any window
  exists and before Silo's setup hook, so a second launch never reaches
  migration, remote management or VM work.
- Every Silo build (debug, release, deb, AppImage) uses the identifier
  `org.silo.preview`, which answers the first open question: all builds share
  one claim, as the owner requires.
- The plugin sends the second launch's `argv` and `cwd`. The running Silo
  resolves `argv[0]` and compares executable bytes (an AppImage mounts at a new
  path on every launch). A different build shows "Silo is already running.
  Quit it first." in the running Silo, which also comes forward; the plugin
  gives the second process no hook to show it itself.
- The Debian update restart replaces the process with `exec`, which skips the
  plugin's exit cleanup, so Silo releases the D-Bus name first.
- Remaining gaps: the macOS socket stays in the shared `/tmp` (no per-user
  path option in the plugin); another local user can still pre-bind it.
  Upstream a configurable socket directory before relying on it for more than
  a single-user Mac.

---

## C-02: remote request locking

**Problem.** One process-wide `REQUEST_LOCK` (`remote.rs:22,852`) is held
across every mutating remote request, including gate waits, retry backoff,
desktop setup (up to 2,100 s) and SSH saves. Queued requests outlive the
controller's 600 s deadline, and retries get fresh request IDs, so both the
stale request and the retry execute (C-02; relates to C-27 replay records).

**Options.**

1. Keep the global lock and add a timeout.
2. Use the existing `operation_gate` per-VM/computer scopes for exclusion. Keep
   a short lock only for the replay-record acceptance step.
3. An external job queue or broker. This is overkill for one host process.

**Decision.** Option 2, plus idempotent operation IDs and an execution
deadline.

- The controller sends `operationId`, which stays stable across auto-retries,
  and `startWithinMs`.
- The bridge records `(operationId → accepted | running | done(result))` under
  the short lock, then releases it.
- If the connection has closed or `startWithinMs` has elapsed by the time the
  gate is acquired, the bridge drops the work and records `expired`.
- A retry with a known `operationId` attaches to the existing status instead of
  executing again.

**Outline.**

1. `remote.rs`:
   - Split `dispatch` into accept, which records under the lock, and execute,
     which runs outside it.
   - Add a per-connection liveness flag (bridge stdin EOF).
2. `runtime/remote_ops.rs`: acquire the VM or computer scope with a
   deadline-aware wait.
3. Controller: reuse `operationId` on retry. Bump the protocol `VERSION`
   (C-18/K-23).

**Tests.**

- Two VMs run concurrently.
- Same VM serialises.
- An expired queued request never executes.
- A retry after timeout returns the first result.
- `2pc` concurrent operations.

**Open question.** How long should completed `operationId` results be kept?
The proposal is 1 h in memory, which is lost on app restart.

---

## C-24: per-sandbox loopback host for published ports

**Problem.** Published ports open at `http://127.0.0.1:PORT`. Cookies are not
port-isolated, so a guest page can read or overwrite non-HttpOnly cookies of
other local services and CSRF them (`network.rs`, `remote_network.rs:254`).

**Options.**

1. Distinct loopback IPs (127.x.y.z).
   - Linux routes all of 127/8 to `lo` (assumed standard).
   - Stock macOS configures only 127.0.0.1, and adding aliases needs root.
     This Mac has extra aliases 127.0.0.10–12 from some other tool, which
     shows they are not a default.
2. `http://<sandbox>-<shortid>.localhost:PORT` while still binding 127.0.0.1.
   - `*.localhost` resolves to loopback per RFC 6761. Modern Chrome and Firefox
     hard-code this (assumed; Safari unverified).
   - Cookies set by the guest are host-only to that name, so they are isolated
     from `127.0.0.1` and `localhost` services.
3. Document the risk only.

**Decision.** Option 2 by default, plus the documentation from option 3.

- CSRF to other loopback services is **not** fixed by any option. Browsers
  treat all these origins as the same local address space. Document this in
  help.
- Keep a "Copy 127.0.0.1 address" fallback for dev servers that reject
  unknown `Host` headers. Some dev servers do; Vite `allowedHosts` behaviour
  for `.localhost` is unverified.

**Outline.**

1. `network.rs` `open_network_port` and `remote_network.rs` build the URL from
   a sanitised DNS label (≤ 63 chars).
2. Frontend shows the same URL.
3. Help text.
4. Unit-test the label sanitising. Run a `mac` check in Safari, Chrome and
   Firefox.

**Open question.** Is Safari support required? If `*.localhost` fails there,
fall back to 127.0.0.1 automatically, or not?

---

## G-04: Unix-socket desktop tunnel

**Problem.** `desktop_viewer.rs:85-118` reserves a TCP port, releases it and
hands it to `ssh -L`. A local process can bind the port first and receive
`Authorization: Basic silo:<password>`. The listener also exposes guest port
6901 to every local user.

**Options.**

1. `ssh -L <0700 dir>/desktop.sock:127.0.0.1:6901` with
   `-o StreamLocalBindUnlink=yes`. The existing `Proxy` then connects with
   `UnixStream`. OpenSSH has supported Unix-socket local forwards since 6.7
   (assumed from OpenSSH release notes).
2. `ssh -W 127.0.0.1:6901` per proxied connection. This means one SSH session
   per HTTP/WebSocket connection, so latency and ControlMaster complexity grow.
3. Keep TCP and verify the peer. Not possible for loopback TCP.

**Decision.** Option 1.

- Put the socket in the existing per-workspace `ssh/desktop-viewer/<ws>`
  directory, which `prepare_private_transport` already creates private (check
  that the mode is 0700).
- Readiness waits for the socket path to exist and accept a connection, and it
  checks that the socket owner is the current uid.

**Outline.**

1. `forward_command` takes a socket path.
2. `Proxy::start` takes a connector trait (TCP for tests, Unix in production).
3. Remove the socket on drop.
4. Remote computers need the same forward on the controller side only; nothing
   changes in the guest.

**Tests.** Unit-test that the proxy forwards over a `UnixListener` fixture and
that a pre-existing foreign socket is refused. Run a `mac` and `linux` desktop
session.

**Open question.** Is any supported controller running an OpenSSH older than
6.7? Unlikely on Ubuntu 24.04 or macOS 14+.

**Implementation note (G-04).** The per-workspace directory is too long for
`sun_path` (104 bytes on macOS including the NUL: `~/.silo/<hash>/ssh/desktop-viewer/`
plus a 32-character name already reaches about 100 for a short home, and remote
workspace identifiers are two UUIDs). Each connection therefore gets a fresh
0700 `~/.silo/desktop-XXXXXX/desktop.sock` directory (`~/.silo` itself is
0700 and owned by the account), removed when the tunnel is reaped. Paths with
`:` or over 103 bytes are refused. The proxy sends `Host`/`Origin`
`127.0.0.1:<guest port>` to the guest, and the system OpenSSH's parsing of the
forward is checked with `ssh -G` in the unit test.

---

## E-19 / E-20: import safety versus msb extraction guarantees

**Problem.**

- The manifest `runtime_config` is validated but not tied to the config inside
  the snapshot. `msb restore` could therefore apply crafted mounts, patches,
  env, image or init (E-19).
- The untrusted `.tar.zst` goes straight to `msb snapshot load`. Its SHA-256 is
  self-declared, decompressed size is unbounded, and the UI says "Verified
  archive" (E-20).

**What msb 0.7.2 guarantees** (from `archive.rs`, read through a summarising
fetch; confirm line by line before relying on it):

| Check | Status |
|---|---|
| Absolute and `..` paths | Rejected ("archive contains unsafe path") |
| Entry types | Checked by `validate_archive_entry_type`. The unpack code carries the comment "Regular / Continuous — the only other types validation lets through", which suggests that symlinks, hardlinks, devices and FIFOs are rejected. The function body was not read. |
| Per-entry blake3 hashes | Checked against the archive's own inventory. The inventory is **self-declared**, with no signature. |
| Aggregate decompressed size or entry count | No limit found |
| Boot overrides | Scope-validated |
| Embedded mounts, env and patches | No validation found |
| `--with-image` OCI layers | No verification found |

**Options.**

1. Trust msb. Rejected, because of the gaps in the table.
2. Pre-scan in Silo with a streaming `tar` + `zstd` reader before `load`, using
   the `tar`/`zstd` crates (assumed not yet direct dependencies). The scan:
   - caps total size (a disk-space-derived limit)
   - caps entry count
   - rejects links, devices and non-regular files. If msb is confirmed to
     reject these already, Silo's check is defence in depth only.
3. Load into an isolated `MSB_HOME` or staging group, inspect the loaded config,
   then promote or discard.
4. Contribute the missing checks upstream: size caps, link rejection, config
   policy hooks.

**Decision.**

- E-20: option 2 now, and option 4 filed upstream. Rename the label to
  "Intact archive".
- E-19: after `snapshot load`, run `snapshot inspect` on the loaded head and
  compare the full restorable config with the validated manifest. Refuse any
  extra mounts, patches, env, image or init.
- Pass every restorable setting explicitly to `msb restore`.
- Add option 3 only if `inspect` does not expose the full config. The v0.7.2
  guide does not document `inspect`'s output fields, so check them against a
  real `msb snapshot inspect` first.

**Outline.**

1. `backup.rs`: add `prescan_archive(path, limits)` before line 601; add an
   inspect-compare after load; clean up the group on refusal (E-23).
2. Rename the label in `backup_controller.rs:329`.

**Tests.** Crafted fixtures:

- symlink entry
- `..` entry
- 10 GB zero-bomb with a small compressed size
- manifest/config mismatch with an added mount
- extra env

**Open questions.**

- Should imports from other people be allowed at all, or should the UI warn
  "only import files you created"?
- Should the image be re-pulled by digest instead of trusting `--with-image`?

**Implementation findings (2026-09-30, WP-E engine).** Read line by line in
the pinned source (`microsandbox-60d4dc8…`), not through a summary:

- `validate_archive_entry_type` (`sdk/rust/lib/backend/local/snapshot/archive.rs`)
  allows only regular, contiguous, GNU sparse and directory entries; GNU
  long names are handled separately and PAX headers are rejected. The table
  above is therefore confirmed. There is still no size or entry-count limit.
- `msb snapshot inspect` prints key/value text (image, scope, root disk,
  parent, labels) without owned volumes or extensions, so it cannot be
  compared with a manifest. The comparison reads the loaded member's
  `snapshot.json` instead (the file `inspect` reads), found through
  `snapshot list --format json` and confined to the native store.
- That descriptor (`packages/microsandbox-types/rust/lib/snapshot/manifest.rs`,
  `deny_unknown_fields`) is the full restorable configuration of a snapshot:
  image reference and digest, root layout, owned volumes, default user and,
  for a full checkpoint, CPU/memory geometry. It has no env, patch, init,
  rlimit or host-mount fields, and `msb restore` binds host resources only
  through explicit flags, which Silo never passes. Passing every setting to
  `msb restore` (option 3) is therefore unnecessary.
- `backup.rs` pre-scans the selected payload with the `tar` and `zstd`
  crates (both already in `Cargo.lock`) in the same read that hashes and
  extracts it: allowed entry types as above, relative normal UTF-8 paths, no
  PAX or long-link headers, at most 262 144 entries, a decompressed-byte
  budget equal to the runtime store's free space less 1 GiB (and less the
  staged copy on a shared volume), and sparse entries no larger than the
  sandbox's largest declared disk.
- Filing the missing caps upstream (option 4) needs an owner with
  MicroSandbox access; it is not done.
- State exports stop at 128 `silo-backup-*` captures per source group. This
  bounds hidden export members without violating the owner's no-automatic-
  deletion decision. The pinned `lineage.rs` commits each successful capture
  as the next capture's parent, so deleting a completed export capture can
  break later checkpoints. Existing checkpoint exports remain available at
  the cap. The cap is conservative product policy; changing it needs no
  archive migration.
- Import cleanup now shares the checkpoint native removal planner. The pinned
  `store.rs` writes `manifest.parent` (a snapshot ID) to the historically
  named `parent_digest` column. Fixtures must use that identity, not an
  archive content digest, to verify child-before-parent removal.
- E-25 export preflight estimates sparse file data from allocated blocks
  in source disks, native snapshots and the image cache, with headers and
  compression overhead. The pinned `archive.rs::save_snapshot` includes
  the parent chain and image-cache files. The estimate includes unrelated
  native data conservatively, sums copies sharing a volume, and leaves
  1 GiB free. It is advisory because sources and free space can change;
  the destination is rechecked with the actual saved payload sizes.

---

## B-05: GitHub permission allowlists for sandbox and host-push tokens

**Problem.** Write mode copies every installation permission, including
`administration`, `secrets`, `workflows`, `repository_hooks`, `actions` and
`environments`. Read mode includes secret-bearing scopes
(`github_tokens.rs:199-221`). Owner decision 1 requires host push to use
`contents: write` only.

**Options.**

1. Denylist.
2. An explicit allowlist, down-scoped per token. Verified in the GitHub REST
   docs for "Create an installation access token for an app":
   - The `permissions` body parameter only down-scopes. A token "cannot be
     granted permissions that the app was not granted".
   - `repositories` and `repository_ids` restrict the token to named
     repositories.

**Decision.** Option 2, using three constant allowlists in `github_tokens.rs`,
each intersected with what the installation grants:

| Token | Permissions |
|---|---|
| Sandbox read | `metadata:read`, `contents:read`, `issues:read`, `pull_requests:read`, `statuses:read`, `checks:read` |
| Sandbox write | read set plus `contents`, `issues`, `pull_requests` and `statuses` at `write` |
| Host push | `contents:write` and `metadata:read` only. Restrict it with `repositories` to the single bound repository, then revoke it after the push (decision 1). The revoke endpoint `DELETE /installation/token` is assumed; it was not re-fetched. |

Never grant `administration`, `secrets`, `dependabot_secrets`,
`codespaces_secrets`, `environments`, `repository_hooks`,
`secret_scanning_alerts`, `security_events` or `actions:write`.

**Outline.**

1. Replace the copy loop with `allowlist.intersect(installation)`.
2. Keep the existing unsupported-permission refusal.
3. Unit-test that each mode never contains a denied scope, and run a `gh` live
   push.

**Open questions.**

- Should sandbox write include `workflows:write`? Without it, agents cannot
  push `.github/workflows` changes.
- Should sandbox read include `actions:read` (CI logs)?

---

## K-23: typed error codes across the native bridge

**Problem.** Errors cross the Tauri bridge as English strings that both sides
match by text (D-17, E-42, H-31, C-18, `production-source.ts:279,469,525`).
Copy changes (decision 10) will break these matches.

**Options.**

1. Tauri's documented pattern: commands return any `Serialize` error, for
   example a `thiserror` enum (already in the dependency graph). This is
   assumed from the Tauri docs.
2. `tauri-specta` for generated TS bindings. It is still RC; adopting it now is
   a large migration.
3. A string prefix convention. Fragile.

**Decision.** Option 1 with a minimal shape:

```rust
struct BridgeError {
    code: ErrorCode,
    message: String,
}
```

- `ErrorCode` is a closed `snake_case` enum: `update_in_progress`,
  `unsupported_remote_operation`, `cancelled`, `already_queued`, `busy`,
  `not_found`, `internal`.
- Keep `From<String>` mapping to `internal` so packages can migrate one at a
  time.
- The remote protocol carries the same `code` field.
- The TS side gets a hand-written union type plus a test that it matches the
  Rust enum.
- Revisit specta after 1.0 of tauri-specta.

**Outline.**

1. `src-tauri/src/bridge_error.rs`.
2. `contracts/bridge-error.ts`.
3. Migrate the five matched sites first.
4. Tests: serialisation round-trip, and a test that fails if a TS code is
   missing from Rust.

**Open question.** None blocking.

---

## K-24: mutex poison policy

**Problem.** `operation_gate` recovers from poisoning with `into_inner`, but
`remote.rs` `CONFIG_LOCK`/`REQUEST_LOCK`, `secrets.rs:15-16`, `github.rs:23,26`,
`network.rs:15` and `editor.rs:18` turn one panic into permanent errors, and
`try_lock` `update_guard`s then block updates forever (B-45, C-25, G-23).

**Options.**

1. `parking_lot::Mutex`, which has no poisoning and is already locked at
   0.12.5. It hides the signal entirely.
2. A std helper `lock_or_recover(&m, name)` that logs once and returns
   `into_inner()`.
3. Leave it per-site. This is the current inconsistency.

**Decision.** Option 2 with a rule:

- **Recover** for `Mutex<()>` serialisation locks and for caches rebuilt from
  disk.
- **Recover and reload** for guards of state persisted to disk (secrets, GitHub
  state, config): after recovery, re-read the file before use.
- Never keep an in-memory invariant across a panic.
- `update_guard` must use `lock_or_recover` semantics instead of treating
  `WouldBlock` from a poisoned lock as busy.
- Replace panicking `unwrap`s under locks (`secrets.rs:415`).

**Outline.**

1. `src-tauri/src/sync.rs` helper, applied by the owning packages.
2. Tests: poison a lock in a test thread and assert that the next call
   succeeds and reloads.

**Open question.** Is parking_lot acceptable instead? It is simpler, but it
loses the log line that a panic happened under a lock.

## Owner answers (2026-09-29)

- **E-03 automatic cleanup:** no. Checkpoints (including "Before restore" and
  "Fork point") are removed only by an explicit Delete checkpoint action or
  when their sandbox is deleted. Dependency-blocked data is removed as soon as
  its last dependent is deleted. Failed or cancelled operations remove partial
  data immediately; crash cleanup at next launch uses their journals, never age.
- **C-24 published-port hosts:** `*.localhost` addresses are acceptable and
  Safari must work. Verify Safari resolution in the macOS live session; if
  Safari cannot open `*.localhost`, fall back to `127.0.0.1` for Safari.
- **E-19/E-20 foreign export files:** importing export files made by other
  people is allowed (no blocking warning). The import safety checks (size and
  entry caps, manifest-versus-snapshot config comparison, "Intact archive"
  wording) still apply.
- **D-02 busy rows:** a sandbox with an operation running keeps its last known
  state next to its existing operation label ("Starting…", "Creating
  checkpoint…"); no extra spinner. Other sandboxes keep updating normally.
- **B-05 workflow and CI permissions:** no separate opt-in. When "Allow GitHub
  changes" is on, the sandbox token also includes `workflows: write` and
  `actions: read`; when it is off, neither is granted. The rest of B-05
  (dropping admin and secret-bearing scopes) is unchanged.
- **F-09 single instance:** only one Silo of any kind runs at a time, including
  development builds next to the installed app. A second launch focuses the
  running Silo, or, when the running one is a different build, shows "Silo is
  already running. Quit it first." instead of crashing.
