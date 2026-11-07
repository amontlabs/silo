# F1 fork measurements, before: Mac (Apple M4 Max)

Run 7 Nov 2026 with `docs/forks-ci/checks/measure_forks.sh` (`MACHINE=mac-m4max-dev`). Full output: `run.txt`; raw files: `raw/`.

- **Machine:** Apple M4 Max, 64 GiB RAM (about 30 GiB free+inactive at start), 16 cores, macOS 26.5 (25F71), arm64.
- **Engine:** Dev engine silod PID 73199, from `app/SiloUI/target/release/bundle/macos/Silo Dev.app` in the main checkout, built at commit 620a4683. CLI `~/.local/bin/silo-dev`. Not rebuilt. Scripts at the commit named in `run.txt`.
- **Source computer:** `main` made by `silo up --recipe demo` (2 cpu, 4 GiB, project `docs/v1/checks/final-proj`); dropped at the end. Other computers untouched.
- **Clock:** monotonic wall clock of the `silo fork` process, spawn to exit (every fork started and `true` run in it). The CLI polls jobs every 300 ms, so a value can read up to 0.3 s late.
- **RAM:** macOS `footprint -p` over the computer's `msb` process tree, 20 s after ready.
- **Cold:** main running and idle, 70 s wait before each run (warm_max_age is 60 s). Checked by counting main's "Fork point" checkpoints around each fork: all three cold runs captured one; all warm runs did not.

## Results (N=3 unless noted; median, min-max)

| | Measurement | Median | Min-max |
|---|---|---|---|
| a | Cold fork of a running idle main | 9.28 s | 8.93-10.49 s |
| b | `fork main -n 4 -- true` right after `silo up` (warm) | 16.01 s | 15.92-16.04 s |
| c | One fork right after `silo up` (warm) | 8.13 s | 7.44-9.56 s |
| d | RAM per fork (12 forks, 3 runs of -n 4) | 498 MiB | 492-506 MiB |
| f | `-n 10` at once (one run): time | 36.76 s | n/a |
| f | `-n 10`: total RAM, 10 of 10 up | 4997 MiB (about 500 MiB each) | n/a |

Reading: a warm checkpoint saves under 1.2 s on a single fork (median 9.28 s cold vs 8.13 s warm; the ranges overlap). Fork time grows with count: 4 forks take about 2x one, 10 about 4.5x. Each fork costs about 0.5 GiB of host RAM while idle.

## Not measured

- **(e) macOS fork to ready:** no macOS computer exists on Dev. `silo create e2e-forks-mac --macos --cpus 4 --memory 8 --disk 60` (from the cached template) ran 5 min and failed with "The computer did not accept SSH logins in time." (`raw/macos-create-attempt.txt`). The computer was dropped. Not retried; needs a working macOS source (set `SILO_MACOS_SOURCE`).
- **Linux KVM host:** the owner's machine was offline. The `forks` job in `.github/workflows/linux-host.yml` runs the same script on the GitHub KVM runner (not yet run).
