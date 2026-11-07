# F1 fork measurements, before: Linux KVM host (GitHub runner)

Run 7 Nov 2026 by the `Fork measurements on KVM` job of `linux-host.yml`, workflow run
[42405253192](https://github.com/amontlabs/silo/actions/runs/42405253192), on the pull request's merge
commit `9d602294` (branch head `9fb86e52`). Artifact `forks-f1-evidence`; full output: `run.txt`; raw
files: `raw/`.

- **Machine:** GitHub-hosted `ubuntu-24.04` runner, Intel Xeon Platinum 8370C at 2.80 GHz, 4 cores,
  15.6 GiB RAM (14.5 GiB available at start), 3.0 GiB swap, Ubuntu 24.04.5, kernel 7.0.0-1017-azure,
  x86_64, `/dev/kvm` present. The runner is itself a cloud VM, so KVM here is nested virtualization; a
  bare-metal Linux host has not been measured (the owner's test machine was offline).
- **Engine:** `silod` built by the job from that commit (Dev channel), CLI from the same build.
- **Source computer:** `main` from `silo up --recipe demo` (2 cpu, 4 GiB).
- **Clock and cold/warm:** as in the [Mac run](../mac-m4max-dev/README.md).
- **RAM:** PSS from `/proc/<pid>/smaps_rollup` summed over the computer's `msb` process tree, 20 s after
  ready.
- The first lines of `run.txt` are an engine-not-yet-started error from the job's preparation; the
  measurements start at `== header`.

| | Measurement | Result (N=3, median, min–max) |
|---|---|---|
| a | Cold fork of a running idle main | 11.03 s (10.70–11.92) |
| c | One fork right after `silo up` (warm checkpoint) | 10.14 s (10.11–10.17) |
| b | `silo fork main -n 4 -- true` right after `silo up` | 36.20 s (35.95–36.21) |
| d | RAM per fork, 12 forks | 1027 MiB (1004–1037) |
| f | 10 forks at once | Not measured: 13774 MiB available, the script needs 14921 MiB |

Reading: as on the Mac, the warm checkpoint saves about 1 s on one fork. Four forks at once take 3.6x
one fork on this 4-core runner (2x on the Mac). PSS per fork is about twice the Mac's footprint figure;
the two tools count memory differently (macOS `footprint` counts the process's dirty and compressed memory, Linux PSS
counts resident pages including guest memory the VM has touched), so the two columns are not directly
comparable.
