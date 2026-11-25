# Step 7 under B: processes in the record

Part of option B's specs ([README](README.md)). A new step, which only B has in the first build: it
builds `processes-in-the-record` for a Linux main, with `processes-from-inside-linux` as its
answer reads. It needs step 2 under B (Silo's kernel) and step 1's recorder. It can be built beside
steps 4 to 6.

#373 counts it in B and leaves it out of A, because under A it needs a probe inside macOS, which
`macos-ships-facts-first` holds back.

## What you can do when it is done

```
$ silo processes main --since 03:00
03:11:58  claude   bash -c 'npm test'                       exit 1     38.2 s   cpu 61 s   rss 1.4 GB
03:12:40  claude   bash -c 'npm test -- auth'               exit 0      4.1 s
01:14:02  claude   node dist/server.js                      running    since 01:14, 2.1 h cpu
02:40:11  codex    rm -rf /home/polarzero/work/shared-fixtures   exit 0   0.3 s
Every line is a report from Silo's probe inside main. An agent with root can stop the probe; when it
falls silent, the record says so.
```

## The pieces

| Piece | Where | What it does |
|---|---|---|
| The kernel's configuration | step 2, `silo-arm64.config` | eBPF on tracepoints, BTF, the process connector, control groups |
| The probe | `probe/linux/`, Zig and one eBPF object, `silo-probe` | Runs in main as a systemd service. Reads process starts and exits, samples their use, sends report lines |
| The probe's way out | vsock port 1024, mapped by libkrun to `run/c1/probe.sock` (step 2) | Not the network: the boundary never sees the probe's lines, and closing main's network leaves them |
| The probe's source | the helper | Reads `probe.sock` and passes the lines to the recorder as the source `probe:c1`, numbered (`sources-number-their-lines`) |

### What the probe reads

From the answer of `processes-from-inside-linux`:

- **Starts and exits** from the kernel's tracepoints `sched_process_fork`, `sched_process_exec` and
  `sched_process_exit`, read by one eBPF program into a ring buffer. Where eBPF cannot be loaded,
  the process connector (`PROC_EVENTS` over netlink) gives the same three events with less detail;
  the probe says which it uses in its first line.
- **The command line** read from `/proc/<pid>/cmdline` at exec, cut at 4 KiB; the full text goes to
  `content/<sha256>` as #373 stores message text, and the line holds the hash and the first 200
  bytes. Secret values in it are handled by the record's redaction (stream 4 of 25 November).
- **Use**, sampled every 10 s from each live process's control group: processor time, memory
  peak, bytes read and written.
- **Groups.** A run is not in the first build. The probe does not ask systemd for scopes yet; it
  records each process's control group path as main has it, which is where a run's scope will sit.

### What a line holds

```json
{"v":1,"seq":30115,"prev":"…","time":"2026-11-25T03:11:58.402Z","computer":"c1","source":{"id":"probe:c1","count":"p:7a0c","n":20441},"is":"report","kind":"process","event":"exit","pid":8812,"ppid":8790,"start":"2026-11-25T03:11:20.190Z","exe":"/usr/bin/node","argv":{"head":"node /home/polarzero/work/app/node_modules/.bin/vitest run","sha256":"…"},"cwd":"/home/polarzero/work/app","uid":1000,"cgroup":"/user.slice/user-1000.slice/session-3.scope","exit":{"code":1},"use":{"cpu_ms":61200,"rss_peak":1468006400}}
```

`is` is always `report`. The time is the helper's, when it read the line, and the probe's own
time is kept as `probe_time`: main's clock is main's.

### Why it stays a report, and what root can do

The probe is a program on main's disk. An agent with root can stop it, detach its eBPF program,
replace it, or send lines of its own on vsock port 1024. Silo's kernel stops some of the means:

| Root in main tries | With Silo's kernel |
|---|---|
| Load a kernel module that hides processes | Refused: `MODULES` is off |
| Replace the kernel at the next boot | Nothing changes: the kernel comes from outside the disk |
| `kexec` a new kernel | Refused: `KEXEC` is off |
| Write kernel memory through `/dev/mem` | Refused: `DEVMEM` is off |
| `systemctl stop silo-probe`, `kill -9` | Works. The helper sees the vsock connection close and the recorder writes the source silent, then a gap when it comes back (`losses-show`) |
| Detach the eBPF program with `bpftool` | Works. The probe's own check of its attachment every second writes a line, if it still runs |
| Send false lines on port 1024 | Works: the port is main's. They are reports, and a rule never reads a report |

The kit's test K15 runs each of these as root and lists what each leaves in the record.

#351 found a way that root could not stop without a kernel exploit: Silo's kernel reports each
exec and exit itself, through a hypervisor call that libkrun's helper receives. It is not what
`processes-from-inside-linux` answers today, and it is code in the kernel and in libkrun to carry.
[decisions.md](decisions.md) proposes it as the probe's next step, under polarzero's direction of
20 November ("as many facts as possible, which cannot be tampered with").

## Cost targets

`own-cost-targets` sets the probe's target. Until it is set, the spec uses #351's question as its
measure:

| Measure | Target |
|---|---|
| A parallel build in main (`make -j8` of a mid-sized C project) with the probe against without | at most 3 % longer |
| Lines from a build that starts 3,000 processes a minute | written whole, with no gap |
| The probe's memory | under 32 MB |

## Tests

| Test | Checks | Where |
|---|---|---|
| The eBPF object loaded against the built kernel's BTF in CI, under QEMU without KVM | It loads and gives the three events | CI on Linux |
| Kit K15: the hostile list above | What each attempt leaves in the record | a Linux host with KVM |
| Kit K16's idle night and K11's night of work with the probe on | The cost target; no gap while the probe runs | a Linux host with KVM, then the Mac |
| #371's claim check with the probe's exits beside the reported ones | A suite's exit 1 is seen twice, once from the traffic and once from the probe | a Linux host with KVM |

## Done when

- every process a night's agents started is in main's record, with its exit, or a gap says when
  the probe was silent;
- a parallel build is slowed by at most the target;
- each hostile attempt of the table leaves the trace the table says.
