# How long a fork takes in Silo v1

> **DRAFT for the owner's approval.** Every number below is copied from
> `docs/v1/evidence/final/final-run.txt` (summary table in `docs/v1/evidence/final/README.md`;
> per-run logs in `docs/v1/evidence/final/raw/`). Nothing is rounded except where marked "about".

Silo forks a running computer so an agent can work on the copy. Here is how long that took on one
Mac, how we measured it, and what we did not measure.

## Results

| What | The clock stops when | Median | Min–max | Runs |
|---|---|---|---|---|
| Linux fork of a running `main`, no usable warm checkpoint | `silo fork` exits: the fork is running and a command has run in it | 9.80 s | 9.47–12.49 s | 12.49, 9.80, 9.47 |
| `silo fork main -n 4` from the warm checkpoint `silo up` saved | `silo fork` exits: all four forks running, a command run in each | 17.06 s | 16.45–19.69 s | 17.06, 16.45, 19.69 |
| macOS fork to ready | `silo fork --gui` returns: the fork is running, its desktop is up and the computer-use driver answers | 59.19 s | 58.97–60.35 s | 60.35, 58.97, 59.19 |

Memory: one Linux fork's VM processes used about 500 MiB, measured with macOS `footprint` 20 s
after the fork was ready; 497.7–502.2 MiB over 5 forks (the first slow-path fork and the four
forks of the first `-n 4` run). `main` was a Linux computer with 2 CPUs and 4 GiB of memory.

## Method

- **Machine:** Apple M4 Max, 64 GiB RAM, 16 cores, macOS 26.5 (25F71). One machine.
- **Build:** a Dev-channel build of commit `09677470` in the release profile, with the same
  runtime binaries a release ships (MicroSandbox 0.7.6 with 24 Silo patches, libkrunfw 5.6.1,
  guest image ubuntu-24.04-v5). It is not the signed production app. The `silo` command is the one
  bundled in that app (release profile, Dev channel).
- **Runs:** N=3 for each time; we report the median with min and max, and every run.
- **Clock:** monotonic wall time of the `silo fork` process from start to exit. Each fork ran
  `true` (`-- true`), so the command returns only after the fork is started and answers a command.
  The command learns that a job finished by polling the engine, so each value can read slightly
  late (the interval is in `docs/v1/checks/final_measure.sh`).
- **Slow path:** `main` running and idle, its last warm checkpoint older than the reuse limit (each
  run waited 70 s first), so the fork first checkpoints `main`, then starts. Images were cached.
- **Fast path:** `silo up --recipe demo` before each run, then `silo fork main -n 4` a few seconds
  later; each run's log says it forked from the warm checkpoint without taking a new one.
- **macOS:** a macOS 26.6.2 source computer, running; each fork made with `silo fork <source>
  --gui`. Two macOS VMs ran at once (the source and the fork).
- **Workload:** a small recipe: a Python web server on port 3000 and a poller service
  (`docs/v1/checks/final-proj/.silo/recipes/demo.yaml`).
- **Scripts:** `docs/v1/checks/final_run.sh`, `final_measure.sh`, `final_macos.sh`,
  `final_footprint.py`, `final_stats.py`.

## Caveats

- One machine, three runs each. Treat the medians as one data point, not a distribution.
- The first run of the slow path (12.49 s) and the third of the fast path (19.69 s) were the
  slow ones; we did not look into why.
- The production app is signed and notarized differently; we did not measure it.
- Memory is what `footprint` reports for the fork's VM processes on the Mac, 20 s after ready, with
  nothing running in the fork but the recipe's web server. It will differ under a real workload.

## Not measured

- `silo up`, including the recipe build (the run logs it but marks it as not a measurement).
- Keep (`silo fold`), undo, Make this main, and `silo diff`.
- Memory of a macOS fork; disk used by a fork or a checkpoint.
- A fork while an agent or a heavy workload runs in `main`.
- Larger `-n` values, other Macs, the signed production app.
- The first run on a new Mac (guest image download).
