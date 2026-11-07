# Open design questions (7 Nov 2026)

Context: [PLAN.md](PLAN.md), facts in [BUILD_LOG.md](BUILD_LOG.md#facts-found-before-planning).
The owner wants the simple design: easy to explain, obvious in hindsight, no special cases.

## Q1. Fork from a saved warm checkpoint by default

Today a fork of a running `main` captures a fresh full snapshot inside the fork job (memory unused),
unless `silo up`'s warm checkpoint is at most 60 s old and main's change generation is unchanged.
Every fork then cold-boots from disks. Candidate designs:

- **A. Fork from the last saved point.** `silo fork main` uses main's newest checkpoint (of any kind,
  or of a "fork base" kind) without capturing; `silo fork main --now` (or `silo checkpoint main`
  first) captures current state. Like branching from a commit. Risk: a fork silently misses edits
  made since the save; needs a clear line ("forked from checkpoint X, saved 12 min ago, main changed
  since").
- **B. Keep "fork = now", make the capture cheap.** Capture disk-only (no memory) when the source
  runs, and keep reusing the warm checkpoint while main's generation is unchanged, dropping the 60 s
  age limit (plain `ssh` edits bypass the generation counter; that gap is why the limit exists).
- **C.** Something simpler.

Also: should warm checkpoints stop saving memory at all (owner decision 3: never carry memory into a
fork)? Is a disk-only capture of a running Linux microVM consistent (MicroSandbox `--guest-flush`)?

## Q2. Forks without a desktop

The desktop (Xvfb, XFCE, Selkies, browser) always starts at boot on guest image v4+. Proposed: a
recipe field `desktop: false` and `silo fork --no-desktop`, applied by setting the guest's desktop
autostart off before the fork's first boot; `fork --gui` and the computer-use tools refuse a computer
without a desktop with a clear message (or start it on demand?). Which default for forks: desktop on
(today) or off unless `--gui`? CI runner forks would use no desktop.

## Q3. Keep an imported computer into my main

Keep is three-way: the fork's fork-time record (base), the fork, the target. An imported computer
has no base, so `silo fold --into main` fails. Candidates:

- **A.** The export carries the computer's fork-time record when it has one (it is a fork); import
  stores it as the imported computer's base, so `fold --into main` diffs the work since the sender's
  fork and checks it three-way against the receiver's main. A computer with no base (a main) can only
  be used as its own computer, not kept into another main.
- **B.** Import records the imported state as the base; nothing to keep until the receiver changes it.
  (Defeats the purpose.)
- **C.** Diff against the receiver's main directly (two-way, every difference counts as the sender's
  work).

## Q4. Guest image by reference in exports

Exports embed the image (`snapshot save --with-image`). Proposed: when the computer's image is the
pinned guest image (`guest-image/image-lock.json` digest), the archive names the digest instead and
import pulls it from the registry (or uses the local cache), refusing with a clear error when offline
and not cached. Any other image stays embedded. Is that a special case to avoid? Alternative: always
embed (simple, larger files).

## Q5. Exporting a computer on another machine

`exports.*` already routes to the owning device but writes the archive to a path on that device.
Options: (a) export there and copy the file back over the Connections SSH link (scp to the
controller), (b) stream the archive through the bridge as chunks, (c) leave it: run `silo export` on
that machine. Which is simplest and safe?

## Q6. CI runner helper shape

A Go helper (`silo-runner`, next to the engine, built on actions/scaleset v0.5.0) with one scale set
per recipe label. Per job: reserve capacity (macOS: 2 slots), `checkpoints.fork` from the recipe's
builder checkpoint (no desktop), `guest.exec.start` of `run.sh --jitconfig` as the runner account,
wait for "Listening for Jobs", on JobCompleted: deregister, then delete the computer on success or
keep it on failure (stopped? running?), with a runner job-completed hook printing the kept computer's
name in the job log. Should the helper speak the socket protocol directly (length-prefixed JSON,
small) or shell out to `silo --json`? Where should the keep-on-failure link come from?
