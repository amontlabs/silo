# MicroSandbox upstream drafts, explained

MicroSandbox (`msb`) runs each Silo computer as a small virtual machine it calls a "sandbox"; a "snapshot" (or "checkpoint") is a saved copy of that machine. Each section below covers one draft: the symptom, what should happen, the smallest MicroSandbox change, and why Silo cares.
Code references are to the pinned tag v0.7.6 in `~/code/projects/msb-upstream/main`; "main" means upstream `origin/main` at bed52046 (2026-11-03), not fetched again. Silo's patches live in `app/SiloUI/patches/`.
Two terms: a "layer" is one disk file stacked on top of another; a "hard link" is a second name for the same file, and its disk space comes back only when every name is gone.

## 1. Old snapshot captures keep their disk space (`msb-reclaim-capture-dirs.md`)

- **What goes wrong:** you take five full snapshots, compact the disk, delete the old snapshots, and the Mac gets no space back. Each full snapshot leaves a folder `runtime/checkpoints/<id>/` holding hard links to the disk layers, and nothing ever deletes it.
- **What should happen:** once a snapshot is saved, its leftover capture folder is deleted, so deleting snapshots frees their disk.
- **Smallest fix:** in `sdk/rust/lib/backend/local/snapshot/create.rs`, remember the capture folder in `capture_full_snapshot` (only when the returned id matches and the source resumed) and delete it after the snapshot or archive is published or fails, the same way disk-only snapshots already do in `SnapshotDiskClosure::drop`. About 20 to 30 lines.
- **Silo's patch adds:** a lock file inside the folder and a sweep at runtime start that removes folders left by a crash. Worth upstreaming as a second step: without it, a crash mid-snapshot still leaks a folder until the sandbox is removed.
- **Why Silo needs it:** Silo takes many snapshots per computer; without this, disk use only grows.
- **Where:** v0.7.6 `crates/runtime/lib/checkpoint/coordinator.rs:365` creates the folder; `create.rs:895-1010` never removes it; `create.rs:105-113` is the disk-only cleanup. Still unfixed on main (folder moved to the sandbox root, `coordinator.rs:377`). Patch: `microsandbox-reclaim-capture-dirs-0.7.6.patch`. Draft is accurate.

## 2. Compacting the disk does not delete the old layers (`msb-retired-layer-cleanup.md`)

- **What goes wrong:** you run `msb modify --compact` to merge many disk layers into one. The merged file is written, but the old layers stay, so the command first uses more disk and gives none back (about 500 MiB in the measured case).
- **What should happen:** after compaction, layers no longer used by the computer are deleted. Snapshots that still need them keep their own hard links, so they are unaffected.
- **Smallest fix:** in `crates/runtime/lib/checkpoint/disk.rs`, `RootDiskCompaction::commit` and `RetiredRootDisk::cleanup`. The cleanup only deletes files under `runtime/`, but the layers sit one folder up. Let it delete retired layers in the sandbox folder, skipping every layer still in the new chain and the launch base. About 10 lines.
- **Silo's patch adds:** a warning when a delete fails. Fine to upstream with it.
- **Upstream changed since:** main (fc5ba184, in v0.7.12) now deletes only files inside `.compact-*` and `.grow-*` folders, on purpose. The layers snapshots create are named `upper-<32 hex>.qcow2` (`next_overlay_path`), so the bug remains. Silo's "anything in the sandbox folder" rule now conflicts with upstream's stated rule; on main the fix should also accept those runtime-made names, still skipping the new chain and the base.
- **Why Silo needs it:** Silo compacts to keep disk use bounded; today that does the opposite.
- **Where:** v0.7.6 `disk.rs:636-660` (parent is `runtime/`), `disk.rs:663-673` (cleanup), `disk.rs:1325-1332` (layer names); main `disk.rs:662-678`. Patch: `microsandbox-retired-layer-cleanup-0.7.6.patch`.

## 3. A stopped sandbox cannot be renamed (`msb-rename-stopped.md`)

- **What goes wrong:** to give a computer another name you have to create a new sandbox and copy everything. The copy gets a new identity and loses its link to past snapshots.
- **What should happen:** `msb rename old new` renames a stopped sandbox in place, keeping its disks, snapshots and history.
- **Smallest fix:** a new CLI command (about 30 lines) and an SDK function in `sdk/rust/lib/sandbox/`: refuse unless stopped, take both names' locks, move `sandboxes/<old>` to `sandboxes/<new>`, update the database row and the stored name, and rewrite the stored absolute file paths (saved config fields and `root-disk.json`). The path rewrite is required: a start refuses a disk record whose layer path does not match. Roughly 200 to 300 lines plus tests.
- **Silo's patch adds:** a crash journal so a rename interrupted halfway is finished or undone at the next rename or start (most of its ~1250 lines). Worth upstreaming, because a half-done rename leaves a sandbox that will not start; it could be a follow-up PR.
- **Rebase note:** main (v0.7.12) checks that every disk layer path is inside the sandbox's own folder and adds a `state-layout.json` record; a rename on main must update those too. The draft does not mention this.
- **Why Silo needs it:** "Make this main" swaps which computer owns a name without losing either one's history.
- **Where:** v0.7.6 `disk.rs:67` (absolute layer path), `disk.rs:868` (start refusal). No rename exists on main. Patch: `microsandbox-rename-stopped-0.7.6.patch`.

## 4. No record of which secrets a sandbox used (`msb-secret-usage-events.md`)

- **What goes wrong:** MicroSandbox swaps a placeholder for a real secret (for example an API key) when the sandbox talks to an allowed website, and blocks it for other sites. Afterwards nobody can tell which secrets were used, or for which sites. A successful use writes nothing, and the "block" setting writes nothing even when it blocks.
- **What should happen:** each use and each block leaves one line naming the secret, the website and the request method, never the secret value or the placeholder.
- **Smallest fix:** in `crates/network/lib/engine/secrets/handler.rs`, emit one structured log event with a fixed name when a header or request line gets a secret (`substitute_in_headers`), and record blocks in `log_violation` even for the "block" setting. The per-sandbox `runtime.log` already exists, so no new file is needed. About 30 to 60 lines; request bodies, HTTP/2 and the GitHub rules add more call sites.
- **Silo's patch adds:** its own `secret-usage.jsonl` file with 1 MiB rotation, a running totals file, a file lock, and cleanup of odd host and method text. Upstream should take the "record successes and silent blocks" part and the rule that values and placeholders never appear; the separate files are Silo's choice.
- **Why Silo needs it:** Silo shows users which credentials each computer actually used.
- **Where:** v0.7.6 `handler.rs:1645-1648` returns early for "block"; the default is "block and log" (`packages/microsandbox-types/rust/lib/domain.rs:2636` on main). Unchanged on main (`handler.rs:1705`). Patch: `microsandbox-secret-usage-events-0.7.6.patch`. Draft is accurate.

## 5. A full snapshot fails after the Mac deletes a shared file (`msb-virtiofs-unlinked-checkpoint.md`)

- **What goes wrong:** the Mac and the computer share a folder (`/.msb` inside). The computer reads a file there, the Mac deletes it, and the next full snapshot fails with "control operation rejected by peer" until the computer restarts or drops its file cache.
- **What should happen:** files that are gone and no longer open are left out of the snapshot; the snapshot succeeds.
- **Smallest fix:** in `crates/filesystem/lib/backends/passthroughfs/unix/mobility.rs`, the macOS `capture_inodes` reopens every file the computer has looked up and fails if one is missing. Skip a missing file when no open handle uses it. About 10 lines plus a test. A file the computer still has open is refused on purpose (`mobility.rs:573-576`), so the draft's ask should be limited to closed files.
- **Silo's side:** no MicroSandbox patch. Silo runs `sync; echo 2 > /proc/sys/vm/drop_caches` in the computer before each full snapshot (`src-tauri/engine/src/runtime/checkpoints.rs:2078`).
- **Already fixed on main:** the draft's "Related: root-disk.json" section. fc5ba184 (v0.7.12) moves `root-disk.json` out of the shared folder (now only `guest-state/` is shared) and checks layer paths against the sandbox's folder. Drop that section before filing. The main bug is unchanged on main.
- **Why Silo needs it:** Silo writes and removes files in that shared folder while computers run.
- **Where:** v0.7.6 `mobility.rs:581` (`open_macos_inode_for_path(...)?`); main only changed size limits in this file (5ddadf86).

## 6. `msb ssh serve` does not say when a connection opens or closes (SSH session markers)

- **What goes wrong:** an editor connects over SSH and changes files, but the program that started `msb ssh serve` cannot tell whether anyone is connected.
- **What should happen:** with an opt-in flag, the server prints one line when a connection opens and one when it ends.
- **Smallest fix:** in `crates/cli/lib/commands/ssh.rs`, `run_serve`'s accept loop: print an "open" line after `accept` and a "closed" line after `serve_connection` returns, behind a new flag such as `--report-sessions`. About 10 lines.
- **Silo's patch adds:** Silo-named lines (`SILO_SSH_SESSION_OPEN`/`CLOSED`) tied to its `--exit-on-stdin-close` flag, plus a task set to collect finished connections. Upstream should use neutral names and its own flag; the task set is optional. It counts connections, not individual shells within one connection.
- **Why Silo needs it:** while a session is open the computer may change, so Silo must not reuse a snapshot taken during it.
- **Where:** v0.7.6 `ssh.rs:229-240` (plain `tokio::spawn`, no output); unchanged on main. Patch hunk: `microsandbox-silo-network-0.7.6.patch` around its line 297; consumer `src-tauri/engine/src/ssh_access.rs:276-286`. A reasonable upstream ask.
