# What keeping the record costs in speed

## The alternatives weighed

Agents decided this entry on 27 November 2026, as `owners-part-on-engine` allows: it fixes how the
cost is measured and nothing you see. A second agent agreed on its own. The check is the benchmark
itself, run with each source on and off, with its numbers in these notes.

An agent's part, 22 November 2026. An agent runs a build with 8 jobs, then a parallel test suite, in
a macOS computer with the record on. Seeing file changes, connections and processes each adds some
time.

The options:

- Measure each source apart (chosen). Builds and parallel test suites run with the record on
  and off, and on a plain machine, in a macOS computer and in a Linux computer. You get one number
  for each thing the record sees, and you see which one costs the time. It costs measuring before
  the limit of 5 percent can be checked.
- One number for the whole record. It is simpler to measure. It costs the choice when the total
  passes 5 percent, since you would not know which source to make lighter.

Chosen: measure each source apart. The trials already found one way of seeing file changes made
a build 1.6 to 2.1 times slower while another added nothing, so the cost sits in the sources.

## Notes

To find out, on a build and on a parallel test suite. How speed is measured is in the part
`checking`.

## A first measure on Linux, inside the computer (24 November 2026)

An agent's part, 24 November 2026. This measures the sources inside a Linux computer, the probe's
process events and fanotify's file events, and not the engine's sources from outside. It ran in
a cloud container (4 processors, Linux 6.18, ext4), not in a Silo computer. The scripts are in
the project's files, under `research/2026-11-24-record-cost/`.

The recorders, written the way a recorder would run them:

- `proc`: eBPF on the scheduler's fork, exec and exit tracepoints, into a ring buffer.
- `files`: `proc`, plus fanotify on the whole filesystem: each closed write with its path read
  back, and each create, delete and move with its name. Every event is written to a record file.
- `opens`: `files`, plus every open, which is what "which process touched which file" needs for
  reads.

The workloads were `go build -a std` (a parallel build, 2 500 processes), `go test` on ten
standard packages (a parallel test suite), and `npm ci` of 8 727 files from a warm cache (many
small files). Each configuration ran 8 times (24 for npm), in a shuffled order. The table gives
medians:

| Workload | Recorder | Time against off | Recorder's own processor time, against the workload's |
| --- | --- | --- | --- |
| build, 16.9 s | proc | +4.5 % | 0.1 % |
| | files | +4.1 % | 0.3 % |
| | opens | −0.1 % | 0.7 % (22 000 opens) |
| tests, 13.0 s | proc | −3.4 % | 0.1 % |
| | files | −0.6 % | 0.2 % |
| | opens | +1.5 % | 0.5 % |
| npm ci, 3.75 s | proc | +1.5 % | 0.1 % |
| | files | +6.6 % | 4.6 % (29 000 events) |
| | opens | +7.6 % | 5.3 % |

- The times move by up to ±5 % with the recorder off, from one run to the next, on a shared
  machine. Their sign does not follow the recorder: `opens` came out faster than `proc` on the
  build. So this run cannot tell a cost under 5 % from none, and the time column only says no
  recorder cost a lot.
- The recorder's own processor time is the firm number. The process events cost almost nothing.
  File events cost in proportion to their count, about 13 µs of the recorder's time each. They
  stay under 1 % on a build or a test suite, and reach 5 % on an install of many small files.
  Inside a computer, the recorder takes that time from the same processors as the work.
- One `npm ci` wrote a 3.2 MB record with opens and 1.75 MB without, about 110 bytes per event, as
  plain text.

So, on Linux, the probe's process events sit far under the 5 % of `record-overhead-limit`, and
file events per process stay under it on a build and a test suite but reach it on a large
install. What still has to be measured is the same in a Silo computer, with the engine's sources
from outside, on machines quiet enough to resolve a few percent, and on macOS.
