# F3 sharing: live check on the Dev engine (7 Nov 2026)

Check script: [`checks/f3_share_check.sh`](../../checks/f3_share_check.sh). Data: live (throwaway `e2e-share-*`
computers on this Mac's Dev engine, all dropped afterwards).

**Build under test:** an integration of `claude/forks-phase-3` and `claude/runner-phase-1` on `origin/main` (merge
commit `f67e2694`), `desktop:build:dev-release`, bundle `~/code/projects/silo-dev-target/release/bundle/macos/Silo Dev.app`.
The engine reported build `silo-f67e2694…` in `~/.silo-dev/engine-owner.json`. launchd refused to start the new
ad-hoc-signed engine (`needs LWCR update`, exit 78, even after booting out the job and reopening the app), so the engine
was started by hand from that bundle (`silod --agent`).

## Result

| Run | Setup | Result |
| --- | --- | --- |
| [run-1-default.txt](run-1-default.txt) (`raw-1-default/`) | as written, base `e2e-share-base` | 18 pass, 4 fail (S7 x3, S9) |
| [run-2-clear-session-bus.txt](run-2-clear-session-bus.txt) (`raw-2-clear-session-bus/`) | `CLEAR_SESSION_BUS=1` | 19 pass, 2 fail (S7 kept, S7 marker), S9 skipped |

Passing in both: S1 (forks), S2 (refused without `--yes`, names the Claude Code login, writes no file), S3 (export with
`--yes`, login listed and kept), S4 (import under a new name, stopped, marker and fake login present, a second import picks
a free name), S5 (`silo diff` works on the imported computer and lists the sender's marker), S6 (fold without `--into`
says it does not know where the computer was forked from), S8 (a conflicting file is refused as divergent and the target
keeps the receiver's version).

### Product findings (not fixed here)

1. **S7: a fold of an imported computer into the receiver's computer is refused on a runtime file.** Run 1: the dry run
   and the keep both refuse with `divergent`: `/home/silo/.dbus/session-bus/<machine-id>-1` "changed on main since the
   fork was made, and differently in the fork". Every started computer rewrites that per-session file, so any two
   computers that have both started since their common ancestor conflict. The ignore list
   (`changes/ignore.rs`) has no entry for it. Probe (run 2, `CLEAR_SESSION_BUS=1` removes the file on the target and the
   copy): the dry run passes (a plan with the marker and conflict files), but the real keep is refused with
   `reason: changed`, "its changes differ from the reviewed diff" (`raw-2-clear-session-bus/s7.json`), so the three-way
   keep into a second computer has not been seen to succeed.
2. **Every export of a fresh computer warns about a login that is not one.** `/etc/pki/ca-trust/source/anchors/microsandbox-ca.pem`
   (the Silo CA every computer has) is named as a login in the S2 warning and in `--json`. `SYSTEM_CREDENTIALS`
   in `ignore.rs` does not list it.
3. **The export free-space check is very conservative.** It counts every snapshot and cache of the engine (also other
   computers') and the computer's source tree, twice (working copy and destination on one volume). It refused a 1 GiB
   export at 31.8 GiB free (needs 39.4 GiB) and a checkpoint export at 38.3 GiB free (needs 76.8 GiB); S9 is therefore
   **skipped** in run 2 (`raw-2-clear-session-bus/s9a.err`), and failed in run 1 for the same reason (the free space was
   31 GiB; the script now keeps the error and prints SKIP for this refusal). S3 itself was refused once at 31.8 GiB free
   (a discarded run), and passed at 38 to 57 GiB.
4. **Imported computers leave their snapshot group behind.** After dropping every e2e computer, six
   `snapshots/silo-import-*` groups remained in the Dev engine (indexed in `snapshot_index`), together 31 GiB on disk
   at the time (`du`, sparse and shared files included) and counted by finding 3. Not removed by hand.

### Export size

| What | Bytes |
| --- | --- |
| A fresh Linux computer (`e2e-share-base`, created, started, nothing installed), lean archive, `silo export --yes` | 2,141,377 (2.0 MiB) |
| A fork of it with a marker file and a fake login (`e2e-share-src`), run 1 | 355,241,422 (339 MiB), 11 s |
| the same, run 2 | 1,097,820,589 (1.02 GiB), 28 s |

The fork's archive is much larger than the base's and varies between runs; the cause was not investigated (the fork's
archive probably includes the parent disk data, which the base's does not; not verified). The same computer exported with the guest image
embedded (the behaviour before F3) was **not measured**: that needs the previous build. The guest image the old format
embedded is 336 MiB (`~/Library/Application Support/org.silo.dev/guest-image`).

### Script fixes made

- `BASE=<computer>` chooses the computer to fork from (default `main`; the Dev engine had no `main`).
- Cleanup stops a computer before dropping it (`silo drop` refuses a running one, so every run left its computers behind
  and the next run's S0 failed on the names).
- S9 keeps the checkpoint export's error and prints SKIP for the free-space refusal.
- `CLEAR_SESSION_BUS=1` (the probe above).

Not run: S9 `--device` (no connected device).

## Run 3 (8 Nov 2026): blocked at S4 by a new import failure

Build: integration of `claude/forks-phase-3` (with the live-check fixes `cf97e65a`) and `claude/runner-phase-1` on
`origin/main`, merge `412e329c`, release-profile Dev bundle, engine started by hand from it (`silo-412e329c…` in
`engine-owner.json`). Base `e2e-share-base`: a new Linux computer (2 cpu), started. Output: [run-3-rerun.txt](run-3-rerun.txt),
`raw-3-rerun/` (JSON and stderr of each step), [raw-3-rerun/probe.txt](raw-3-rerun/probe.txt).

| Expected | Seen |
| --- | --- |
| S2 names only the Claude Code login, no CA-cert login | **Pass.** The warning lists `/home/silo/.claude/.credentials.json` only (`raw-3-rerun/s2.err`) |
| S9 checkpoint export runs | **Pass.** It exported by name; no free-space refusal at 59 GiB free (S9 `--device` still skipped) |
| S3 export with `--yes` | Pass, 3 s, 7,865,399 bytes (7.5 MiB) for the fork with a marker and a fake login |
| S4 import under a new name | **Fail.** `silo: Import failed: Loading computer disks from the export file failed: error: archive contains duplicate entry: layers/layer_87157fb508a8231822a4fcead2aa2985.raw No computer was added.` |
| S5 to S8 (diff of the copy, fold, S7 keep into the receiver's computer, conflict) | **Not reached**: every one needs the imported computer. Their FAIL lines are consequences of S4, not findings; S7 (keep into the receiver's computer) was therefore not seen to pass |
| no `snapshots/silo-import-*` left after dropping everything | The failed imports added none (the directory list is identical before and after the run), but the **six groups from the earlier runs are still there** (8.4 GB, `du`), and starting the new engine did not clear them |

**Product problem (fixed after run 3, see Run 4).** The import failure is not specific to the fork: a never-forked, freshly created and
started computer (`silo export e2e-share-base -o base.silo-backup --yes`, 6.4 MiB) fails to import with the same error and the same layer name,
and so does a stopped fork (`probe.txt`). The message comes from the runtime's `snapshot load` (`backup.rs`,
`load_import_group`), so the exported payload contains that layer twice; the same shape passed S4 in runs 1 and 2 on the
previous build (`f67e2694`). Between those runs and this one the branch gained only `cf97e65a` (a smaller export estimate,
import-group clean-up and ignore-list changes), and the exports are now 7 to 12 MiB where they were 2 MiB to 1 GiB, so
what goes into the payload changed. Not investigated further (which entry is duplicated, whether the export side adds the
image layer twice, or whether the runtime's own base-image layer is referenced by two members).

The three finding-fixes of `cf97e65a` that could be seen without an import: the CA is no longer a login (S2), the
space check no longer refuses (S3 and S9 ran at 59 GiB free), and a failed import leaves no group. The clean-up of
groups left by an *imported computer that was dropped* could not be tested.

All e2e computers were dropped. The probe archives were in `/tmp/f3-probe` (deleted).

## Run 4 (8 Nov 2026): S1 to S9 pass, no import group left

Build: integration of `claude/forks-phase-3` (with `8794f629`, `5aa8a1ae`, `05992ceb`) and `claude/runner-phase-1` on
`origin/main` (local branch, never pushed; merge `3a085526`), release-profile Dev bundle, engine started by hand from it
(`silo-3a085526…`). Base `e2e-share-base`: a new Linux computer (2 cpu), started. Output: [run-4-rerun.txt](run-4-rerun.txt),
`raw-4-rerun/`. An earlier attempt on the build before the last fix is kept as
[run-4a-before-shell-fix.txt](run-4a-before-shell-fix.txt) (22 passed, 0 failed too; two empty import folders stayed after the run).

**22 passed, 0 failed**, S9 `--device` skipped (no connected device). S4 imports, S5 `silo diff` of the copy lists the sender's
marker, S6, S7 (the keep into the receiver's computer, `e2e-share-target`) **passes** (dry run, keep, marker in the target), S8.
The export of the fork with a marker and a fake login: 5,314,579 bytes (5.1 MiB), 2 s.

### Cause of the run 3 import failure

MicroSandbox's `snapshot save --with-parents` writes a layer twice when the capture's own layer chain already contains its
parent's head, and `snapshot load` refuses such an archive (`archive contains duplicate entry: layers/layer_<hash>.raw`).
That is the case for a capture taken after an earlier capture of the same computer: its disk is a qcow2 layer backed by the
raw layer that is the parent's head. Reproduced on the Dev engine with one fresh computer (`silo export` four times, importing
each file): the first three imported, the fourth failed with that error; saving the same capture by hand
(`msb snapshot save <capture> out.msb`) with `--with-parents` gave an archive with `layers/layer_eda7….raw` listed twice
(`zstd -dc | tar t | sort | uniq -d`), without it an archive that loads. Runs 1 and 2 never exported a computer whose
lineage already held a capture, so they did not see it; in run 3 the forks of S1 (F2's disk-only fork points) had put captures
on the base before the export. Fix: an export saves the capture alone (`backup.rs`); the capture holds every layer, and the
retry path for a missing ancestor already relied on that.

### The old `silo-import-*` groups

By the time they were inspected (after run 3) the six groups held no snapshot (`group.json` with `"head": null`, `.group.lock`),
4 KB each; the 8.4 GB in the run 3 note was not reproduced. What was left was the empty folder. Two causes:

1. Nothing called the sweep. `recover_interrupted` (which queues the members of import groups no checkpoint record names)
   is only called by `maintenance_tick`, which only `storage::start_monitor` calls, and no code has called `start_monitor`
   since `ff4fe89f7` (Silo.app stopped starting an engine). `silod` now has a storage timer (30 s after start, then every ten
   minutes) that runs the recovery; live, the nine empty folders were gone about 30 s after the new engine started.
2. Neither the sweep nor a drop removed the folder after the last member went. The sweep now removes the empty
   folders of unowned import groups, and any removal of queued members removes the group's empty folder. An imported copy of a
   computer keeps the snapshot ID of the computer it was exported from, so it stays while that computer lives (seen live: an
   import of the base's own export stayed until the base was dropped); the folder now goes with it at that point.

After dropping every `e2e-share-*` computer including the base: no `silo-import-*` folder in `snapshots/`, no such entry in
`msb snapshot ls`.
