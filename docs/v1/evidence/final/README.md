# Silo v1 final run

Engine and CLI at commit `09677470` (printed in the header of `final-run.txt`), built with
`build_desktop.py --dev-release` (release profile, Dev identity; same runtime binaries as a release).
The CLI is the `silo` bundled in `Silo Dev.app`; it reports `silo 0.1.0 (release profile, debug
assertions on, Dev channel)`. Machine: Apple M4 Max, 64 GiB RAM,
macOS 26.5. MicroSandbox 0.7.6 (source 09df3d4b9d83) with 24 Silo patches; guest image ubuntu-24.04-v5.

| File | Sections | Result |
|---|---|---|
| `final-run.txt` | header, demo, quiet, computer use, promote, cold, warm `-n 4`, macOS | 25 passed, 0 failed |
| `final-run-claw.txt` | OpenClaw recipe (needs a fresh `main`), same build | 9 passed, 0 failed |
| `history/final-run-8f764a38.txt`, `history/claw-8f764a38.txt` | the previous full run, before the fixes (commit in each name) | superseded |
| `final-run-part3-demo.txt`, `final-run-part2.txt`, `final-run-pass1.txt`, `try*.txt` | earlier attempts, kept as history | — |

Measurements (all from `final-run.txt`, N=3 each, median with min–max):

| What | Clock stops when | Median | Min–max |
|---|---|---|---|
| Linux fork of a running `main`, cold (no usable warm checkpoint) | `silo fork` returns with the fork running and ready | 9.80 s | 9.47–12.49 s |
| `silo fork main -n 4` from the warm checkpoint | all four forks running and ready | 17.06 s | 16.45–19.69 s |
| macOS fork to ready | `silo fork --gui` returns with desktop and computer use ready | 59.19 s | 58.97–60.35 s |
| RAM of one Linux fork (`footprint` of its VM processes, 20 s after ready) | — | about 500 MiB (497.7–502.2 MiB over 5 forks) | — |

Scripts: `docs/v1/checks/final_run.sh` and `final_*.sh` beside it.
