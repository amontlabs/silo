# Option B's test kit, for a Linux host with KVM

Tests for option B ([README](../README.md)) that run before the Mac does: libkrun is the same
library on a Mac (Hypervisor framework) and on Linux (KVM), so every question about main's
kernel, image, seed, way in, network, checkpoints and reader can be answered on a Linux host
first. What only a Mac can answer is listed at the end, as tests for #359's kit.

None of these tests has run: the container this was written in has no `/dev/kvm` (#366 found the
same). `krun-boot.c` was checked to compile against libkrun 1.19's header. `tools/prefix.py`,
`../proto/ext4read.py`, `../proto/qcow2raw.py` and `../proto/check.sh` did run; their results are
in [../proto/results.txt](../proto/results.txt).

## What it needs

- Linux on arm64 with KVM, so main's architecture is the Mac's: a rented Ampere or Graviton metal
  machine, or a Linux machine of your own. An x86 host runs every test with an x86 kernel and
  image; the kernel's options are the same.
- libkrun 1.19 installed with its header (`dnf install libkrun-devel` on Fedora; from source
  elsewhere), gvproxy, socat, genisoimage or xorriso, e2fsprogs, a compiler and what the kernel
  build needs (flex, bison, bc, libelf, pahole).
- `$KIT` (default `~/silo-kit`) on XFS or Btrfs, for reflink clones: they stand for APFS's
  `clonefile`. Tests that need them skip otherwise.
- Root, through `sudo`, for K05, K11 and the `proto` checks (loop devices and mounts of copies,
  never of a running disk).

## Running it

```
sh setup.sh                 # builds krun-boot and Silo's kernel, fetches and converts Ubuntu 26.04
sh tests/K00-host.sh        # then each test, in order; each appends a line to $KIT/results.jsonl
```

Each test starts with a header in the form of #359's kit: `id`, `title`, `needs`, `decides` (the
plan's entries it informs), `question` and its outcomes, so stream 6 can fold it into that kit.

## The tests

| Id | Question | Informs |
|---|---|---|
| K00 | The host has KVM, libkrun, the kernel and the image | |
| K01 | Silo's kernel boots Ubuntu through libkrun; first and second boot times; systemd `running` with no modules | `linux-vm-engine` (1), `computer-image`, `blank-computer` |
| K02 | Docker runs, with a bridged container | `linux-vm-engine` (3), `containers-on-macos` |
| K03 | SSH over vsock, with no network device at all | `network-before-rules`, the way in |
| K04 | What main's network reaches through gvproxy: the host's loopback, gvproxy's API, the LAN | `network-stack`, #358, #363 |
| K05 | An orderly stop of a busy main; a power cut during writes, then boot and `e2fsck` | `what-stop-ends` |
| K06 | A cold fork from a clone of a running main | `linux-vm-engine` (2), `fork-shares-disk-blocks` |
| K07 | A clone of the running disk holds a prefix of main's completed writes | `checkpoint-per-system` |
| K08 | The longest write in main during checkpoints, `clone` against `delta` | `speed-budgets`, when B moves to `delta` |
| K09 | A restore to a checkpoint taken during `npm install` boots, ext4 replays its journal | `restore-to-a-checkpoint` |
| K10 | A deleted file's space comes back to the host after `fstrim` | `space-the-engine-keeps` |
| K11 | The reader's diff of each pair of a night's checkpoints equals the kernel's | `file-changes-from-outside` |
| K12 | fscrypt on a folder gives a gap | `file-changes-from-outside` |
| K13 | What root in main can do: modules, kexec, `/dev/mem`, replacing `/boot`'s kernel | `processes-from-inside-linux`, `storage-between-computers` |
| K14 | #359's test 34 in its Linux form, once the boundary of step 6 exists | `model-traffic-read` |
| K15 | The probe's means in Silo's kernel (BTF, tracepoints) and their cost on a parallel build | `processes-from-inside-linux`, `own-cost-targets` |
| K16 | What Ubuntu writes by itself on an idle night | `linux-noise.zon` |

## What only the Mac can answer

To add to #359's kit. Each runs `krun-boot` built against Homebrew's libkrun on the Mac.

| Id | Question | Informs |
|---|---|---|
| L1 | Does the helper boot main on HVF on the oldest macOS it should support (14), and on 26.5? Does libkrun's stop request power main off with the seed's `HandleRebootKey=poweroff`? | `linux-vm-engine` (1), `mac-silo-needs` under B |
| L2 | How much memory an idle main gives back after freeing 4 GiB | `linux-vm-engine` (4) |
| L3 | Disk speed in main against the Mac's own (fio, random and in order) | `linux-vm-engine` (5), `speed-budgets` |
| L5 | K07 on APFS: is a `clonefile` of `disk.raw` taken between two of libkrun's `pwrite`s a moment in time? | `checkpoint-per-system` |
| L6 | The write log's cost per 4 KiB write, patched libkrun against unpatched | `libkrun-patches`, step 4 |
| L7 | Does a helper run main while you are logged out, as a launch daemon? | `engine-matches-wanted` |
| L8 | K10 on APFS: does a discard free `disk.raw`'s blocks? | `space-the-engine-keeps` |

#359's tests 03, 05, 06, 08, 09, 16, 20, 31 and 33 apply under B unchanged or on `disk.raw` (05
for the `clone` strategy's comparison); 02 in its Linux form. Its tests 00, 01, 04, 07, 10, 11, 12,
13, 14, 15, 17, 18, 19, 21, 22 and 32 are about a macOS main and are not needed for B's first
build. Test 34 runs in its Linux form, first as K14. Its tests 23 to
30 (Xcode, simulators, signing, an iPhone) are what B gives up.
