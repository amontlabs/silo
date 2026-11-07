# F2 fork measurements, after: Mac (Apple M4 Max)

Run 7 Nov 2026 with `docs/forks-ci/checks/measure_forks.sh` (`MACHINE=mac-m4max-dev`), the same script and method as [f1-before](../../f1-before/mac-m4max-dev/README.md) except the changes below. Full output: `run.txt`; raw files: `raw/`; `desktop-toggle.txt` is the live check of `silo desktop`.

- **Machine:** Apple M4 Max, 64 GiB RAM (about 26 GiB free+inactive at start), 16 cores, macOS 26.5 (25F71), arm64. Same Mac as F1.
- **Engine:** Dev engine `silod` from `/Users/polarzero/code/projects/silo-dev-target/release/bundle/macos/Silo Dev.app` (release-profile Dev bundle of branch `claude/forks-phase-2`), engine build `silo-eeeb49b6` for the timed run (commit `eeeb49b6`; the desktop-on fix `b696d9e3` came after and does not touch fork paths). CLI: the `silo` in that bundle, not the production one. Started by hand (see "Not done").
- **Source computer:** `main` made by `silo up --recipe demo` (2 cpu, 4 GiB), dropped at the end. Other computers untouched.
- **Clock and RAM:** as in F1 (wall clock of the `silo fork` process; `footprint -p` over the computer's `msb` process tree 20 s after ready).
- **What changed in the script:** no 70 s wait before a cold run (there is no warm checkpoint to outlast), the project is `docs/forks-ci/checks/proj` (the demo recipe, plus `demo-plain` and `demo-nodesk`), and sections `plain` and `nodesk` were added. Sections `cold` and `warm1` now run the same path; both are kept so the rows line up with F1.
- **Capture check:** after a fork of a running `main`, the engine's record for the "Fork point" checkpoint says `scope: disk` (`runtime/checkpoints/<id>.json`); MicroSandbox's own log of a running computer forked by hand during the run (a `demo-nodesk` computer, not `main`) says `capture_disk ... pause_us=81829 total_us=98314` (the guest is paused about 0.1 s while its disk is sealed). F1's capture was a full memory and disk snapshot.

## Results (N=3 unless noted; median, min-max)

| | Measurement | F1 before | F2 after |
|---|---|---|---|
| a | One fork of a running idle `main` | 9.28 s (8.93-10.49) | **8.47 s** (8.39-9.65) |
| c | One fork right after `silo up` | 8.13 s (7.44-9.56) | 8.63 s (8.63-8.64) |
| b | `fork main -n 4 -- true` right after `silo up` | 16.01 s (15.92-16.04) | 17.78 s (17.42-18.34) |
| d | RAM per fork (12 forks, 3 runs of -n 4) | 498 MiB (492-506) | 497 MiB (494-499) |
| f | `-n 10` at once: time | 36.76 s | 37.77 s |
| f | `-n 10`: total RAM, 10 of 10 up | 4997 MiB | 5010 MiB |
| g | One fork of a recipe computer with a desktop, no services (`demo-plain`) | n/a | 8.61 s (8.40-8.82); RAM 487 MiB |
| h | One fork of the same recipe with `desktop: false` (`demo-nodesk`) | n/a | **7.76 s** (6.97-8.36); RAM **211 MiB** |

Reading:

- A fork of a running source is about 0.8 s faster than F1's cold fork (a). The cost of the capture itself is small: the disk-only snapshot pauses the guest for about 0.1 s; the rest of a fork is the cold boot, the quiet step and the CLI's command.
- The rows that used the warm checkpoint in F1 (c, b, f) are 0.5 to 1.8 s slower, because there is no warm checkpoint to skip the capture with: (c) +0.5 s, (b) +1.8 s, (f) +1.0 s. This is what the owner accepted (decision Q1): F1 measured the warm checkpoint as worth about 1 s on a single fork.
- RAM per fork with a desktop is unchanged, about 0.5 GiB.
- Without a desktop a fork uses **211 MiB** against 487 MiB with one (-57%) and is ready 0.8 s sooner (7.76 s against 8.61 s). (g) and (h) use the same recipe without its two services (a Linux service starts with the desktop session, so `desktop: false` cannot have services); the only difference between them is `desktop: false`.

## Found and fixed during the run

- A computer with its desktop off held its operation lane for about 5 minutes after every start: the guest's computer-use setup waits up to 300 s for a desktop session that never comes, and a fork's capture waits for the lane (the first `nodesk` fork took 288 s). The engine now skips the computer-use setup at boot and at creation when the desktop is off, and applies it when the desktop is turned on (`desktop-toggle.txt`: computer use is `ready` seconds after `silo desktop on`). The numbers above are from the build with that fix.

## Live check of the desktop setting (`desktop-toggle.txt`)

`silo status` says `<name> has no desktop; turn it on with silo desktop on <name>`; `silo fork <name> --gui` refuses with the same sentence; `silo desktop on` turns it on and computer use is `ready` a few seconds later; `silo desktop off` stops the desktop; a fork inherits the setting of its source (`desktop = True` and `False` by source).

## Not done

- **macOS:** no macOS computer exists on Dev (see F1), so a macOS fork was not timed. In the code a macOS fork point is already disk-only (`capture_held` with `PolicyKind::ForkPoint` syncs the guest and copies the disk) and `desktop: false` is refused for a macOS recipe.
- **Linux KVM host:** the owner's machine was offline; the `forks` job in `linux-host.yml` runs the same script on the GitHub KVM runner (not run).
- **launchd:** the Dev engine from this bundle was started by hand (`silod --agent`), not by its LaunchAgent. launchd's job for `org.silo.dev.engine` was registered from the previous bundle and fails to spawn the new one (`last exit code = 78`, "needs LWCR update"); quitting and reopening `Silo Dev` from this bundle, with the job booted out, registers it again.
