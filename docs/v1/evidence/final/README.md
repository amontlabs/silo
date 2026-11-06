# Silo v1 final run

Engine and CLI at the commit printed in each file's header, built with `build_desktop.py --dev-release`
(release profile, Dev identity; same runtime binaries as a release). Machine: Apple M4 Max, 64 GiB RAM,
macOS 26.5. MicroSandbox 0.7.6 (source 09df3d4b9d83) with 24 Silo patches; guest image ubuntu-24.04-v5.

| File | Sections | Result |
|---|---|---|
| `final-run.txt` | header, demo, quiet, computer use, promote, cold, warm `-n 4`, macOS | 25 passed, 0 failed |
| `final-run-part4-claw.txt` | OpenClaw recipe (needs a fresh `main`) | 9 passed, 0 failed |
| `final-run-part3-demo.txt`, `final-run-part2.txt`, `final-run-pass1.txt`, `try*.txt` | earlier attempts, kept as history | — |

Measurements (all from `final-run.txt`, N=3 each, median with min–max):

| What | Clock stops when | Median | Min–max |
|---|---|---|---|
| Linux fork of a running `main`, cold (no usable warm checkpoint) | `silo fork` returns with the fork running and ready | 8.87 s | 8.62–10.75 s |
| `silo fork main -n 4` from the warm checkpoint | all four forks running and ready | 15.57 s | 15.53–17.67 s |
| macOS fork to ready | `silo fork --gui` returns with desktop and computer use ready | 51.69 s | 51.68–57.84 s |
| RAM of one Linux fork (`footprint` of its VM processes, 20 s after ready) | — | about 500 MiB (499.0–507.1 MiB over 5 forks) | — |

Scripts: `docs/v1/checks/final_run.sh` and `final_*.sh` beside it.
