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

**Product problem, not fixed here.** The import failure is not specific to the fork: a never-forked, freshly created and
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
