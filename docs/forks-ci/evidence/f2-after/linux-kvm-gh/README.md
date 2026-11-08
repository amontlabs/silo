# F2 fork measurements, after: Linux KVM host (GitHub runner)

Run 7 Nov 2026 by the `Fork measurements on KVM` job of `linux-host.yml`, workflow run
[42419819528](https://github.com/amontlabs/silo/actions/runs/42419819528), on PR #88's merge commit
`e88eeb5c` (branch head `f72aac2d`). Artifact `forks-f1-evidence`; full output: `run.txt`; raw files: `raw/`.

- **Machine:** GitHub-hosted `ubuntu-24.04` runner, Intel Xeon Platinum 8370C at 2.80 GHz, 4 cores,
  15.6 GiB RAM (14.5 GiB available at start), Ubuntu 24.04.5, kernel 7.0.0-1017-azure, x86_64, nested KVM.
  The same runner type as [f1-before](../../f1-before/linux-kvm-gh/README.md), but not the same machine
  instance or time; bare-metal Linux is still unmeasured.
- **Method:** as in [f1-before](../../f1-before/linux-kvm-gh/README.md). The F2 script waits 0 s before a
  "cold" run, since there is no warm checkpoint any more. `plain` and `nodesk` are the `demo` recipe and the
  same recipe with `desktop: false`, one fork of each source per run.

| | Measurement | Before (F1) | After (F2) |
|---|---|---|---|
| a | One fork of a running idle main | 11.03 s (10.70–11.92) | 11.71 s (11.66–11.95) |
| c | One fork right after `silo up` | 10.14 s (10.11–10.17) | 11.95 s (11.95–11.96) |
| b | `silo fork main -n 4 -- true` | 36.20 s (35.95–36.21) | 41.08 s (39.71–43.15) |
| d | RAM per fork (PSS, 12 forks) | 1027 MiB (1004–1037) | 1027 MiB (1008–1038) |
| f | 10 forks at once | not measured (memory) | not measured: 13820 MiB available, 14879 MiB needed |
| g | One fork with a desktop (`plain`) | n/a | 14.29 s (13.92–14.36), 1022 MiB |
| h | One fork without a desktop (`nodesk`) | n/a | 8.62 s (8.49–8.82), 387 MiB |

N=3, median (min–max); RAM rows g and h are one fork each, 20 s after ready (`run.txt` lines 142-170).

Reading: on this runner the forks that used the warm checkpoint in F1 (rows b and c) are 1.8 to 4.9 s
slower, and a fork of a running main is 0.7 s slower; the two runs are on different runner instances hours
apart, so part of that difference may be the machine. A fork without a desktop takes 40% less time and 62%
less memory than the same fork with one.
