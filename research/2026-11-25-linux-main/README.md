# Option B ready to decide: a Linux main on the Mac

An agent's work, 25 November 2026, for polarzero and for the agents who will build. #373 compares
first builds and recommends A, a macOS main with the record of model traffic, with B, a Linux main
on the Mac, as the fallback without specs. This folder specifies B at the level #370 specifies the
macOS main, so either can be taken the day the Mac's tests answer. It changes no verdict.

## What was found

1. **B's Linux computer is not extra work.** The plan needs it anyway for
   `computers-linux-beside-main`: a libkrun helper, Linux checkpoints, a reader of ext4. Under A it
   comes in the third build; under B it comes first. So the first code can be the same for both:
   #370's step 1, this folder's step 2 as a Linux test computer, #373's step 6 against it. The
   choice between A and B can wait until the Mac kit's tests 00, 01, 07 and 21 have run
   ([a-and-b.md](a-and-b.md)).
2. **A copy of a running ext4 disk hides its last changes until the journal is replayed.**
   Copies taken 7 s and 30 s after a burst of changes showed none of them to a reader that ignores
   the journal; at 120 s they all showed. A trial reader that replays the journal in memory matched
   the kernel exactly, on live copies and on Ubuntu 26.04's own image (107,853 paths)
   ([proto/results.txt](proto/results.txt)). Without it, file facts land in the wrong window.
3. **Under B Silo sees every write of main**, in one function of libkrun's block device. A hook
   there (a patch shaped for upstream) gives #365's `delta` checkpoints, which kept writes under
   160 ms where clones made them wait 9 to 12 s, and tells the comparer exactly which blocks changed.
   Under A the same needs main's disk served over NBD, which nobody has shown a macOS computer boots
   from.
4. **B removes A's blocking Mac unknowns**: no macOS 27 on the Mac (libkrun's floor is macOS 14, to
   confirm), no provisioning (cloud-init sets up Ubuntu), no gvproxy under a macOS computer (Podman
   runs gvproxy with libkrun on a Mac), no Keychain over SSH (Linux's store, as #347 ran). Its own
   unknowns are Silo's kernel booting Ubuntu with no modules, and the helper on HVF.
5. **The way in over vsock** keeps SSH when main's network is closed, from the first build.
6. **Silo ships main's kernel**, which main cannot replace or extend (no modules, no kexec, no
   `/dev/mem`), with what the probe needs (eBPF on tracepoints, BTF), which the stock libkrunfw
   kernel lacks (#351).
7. **Ubuntu 26.04's image is 943 MB with 95 % of its clusters compressed**, so the helper's
   converter has to inflate them; a trial converter did it in 8.6 s.
8. **Counted again on #348's tree, B needs 96 open decisions and A 87** (#373: 94 and 88). B keeps
   `files-through-a-channel` and `mac-silo-needs`, which #373 dropped.

## The pages

| Page | Holds |
|---|---|
| [a-and-b.md](a-and-b.md) | A and B side by side: what each needs before the first night, the first morning under each, what carries over, and which to take |
| [decisions.md](decisions.md) | Second opinions on the 26 decisions B adds or changes that #352 and #373 did not cover, with the text to take |
| [2-main-runs.md](2-main-runs.md) | Step 2: the Zig helper on libkrun, Silo's kernel, Ubuntu's image, the first boot's seed, the way in over vsock, the network, stop and pause |
| [3-a-night.md](3-a-night.md) | Step 3: #370's, with the differences in the helper |
| [4-the-way-back.md](4-the-way-back.md) | Step 4: clone first, then `delta` through the write log; restore; discards |
| [5-the-record.md](5-the-record.md) | Step 5: the ext4 reader with the journal replayed, comparison from the changed blocks, file facts with inode and generation |
| [6-model-traffic.md](6-model-traffic.md) | Step 6: #373's, with the authority in Linux's store and the boundary in the helper |
| [7-processes.md](7-processes.md) | Step 7, only under B: the probe, what root can do to it |
| [kit/](kit/README.md) | 17 tests for a Linux host with KVM (K00 to K16), and 7 for the Mac (L1 to L8) to add to #359's kit |
| [proto/](proto/) | The trial ext4 reader, the qcow2 converter, and their checks against the kernel, which ran |
| [tools/count.py](tools/count.py) | The count of open decisions for A and B |

Steps 1 (the engine) and the formats are #370's, unchanged apart from the list at the end of
[2-main-runs.md](2-main-runs.md).

## What it changes in the plan

Agents' notes, no answer, `sets` or verdict changed:

- New feature `computers-linux-main-on-a-mac`, undecided, in the stage "Later" after
  `main-on-linux`, with its choice. Suggested: drop for now, as the fallback. On #348's tree it
  should also need `mac-silo-needs` and `network-before-rules`, as `computers-main-on-a-mac` does
  there.
- Notes on `main-on-macos-first`, `main-on-linux`, `linux-vm-engine`, `checkpoint-per-system`
  (allow a checkpoint made of the blocks copied before they are overwritten),
  `file-changes-from-outside` (replay the journal), `processes-from-inside-linux` (the kernel's own
  report as the next step), `computer-image` (pin the raw disk's hash too).

`plan check` passes.

## What was run, and what was not

- **Ran**, in a Linux container with no KVM: the ext4 journal measurements, the trial reader against
  the kernel on live copies and on Ubuntu's image, the qcow2 converter on Ubuntu's image, the
  prefix check of test K07 on a planted tear, and the count of decisions.
- **Not run**: every test of the kit that boots a computer. The container has no `/dev/kvm`.
  `krun-boot.c` compiles against libkrun 1.19's header; it was never linked or started. Building and
  running libkrun's own tests here was refused by this session's permissions, so the write log
  patch is specified, not written.
- **Read, not run**: libkrun's `main` and `stable-1.19.x` headers and its block device's source, on
  25 November 2026.
- **Assumed, to confirm**: libkrun's floor of macOS 14 on a Mac (L1); that libkrun's stop key powers
  main off with the seed's logind setting (L1); that a `clonefile` of `disk.raw` taken between two
  of libkrun's writes is a moment in time (L5); main's clock after the Mac sleeps.

## The open entries these specs assume

Where B needs an answer other than #370's or #373's. Every other entry is as #370 and #373 assume.

| Entry | B assumes | Other options, and what changes |
|---|---|---|
| `main-on-macos-first` | "Linux first" (B's own premise) | macOS first: A, #370 and #373 |
| `mac-silo-needs` | Apple silicon, macOS 14 or later | A's macOS 27: no change to B's code, only the first run's check |
| `linux-vm-engine`, `libkrun-patches` | libkrun's 2.0 interface pinned to a commit, Silo's patches on top | 1.19: no pause; `pause-every-computer` waits for 2.0 |
| `checkpoint-per-system` | Clone first, `delta` once the write log passes review | Clone only: the write-wait target fails on a fragmented image, as under A |
| `processes-from-inside-linux` | The eBPF probe, a report | The kernel's own report through the helper: step 7's probe gives way to a kernel patch |
| `which-process-holds-the-boundary` | The boundary as a library in the helper | A second process in the helper's job, as #373 reads it: one more launchd child |
| `main-set-up-for-agents` | git, tmux, build tools, Node.js, the agent programs | A blank main: step 9 of the first run goes |

## For the README's "research at a glance"

> Option B (Linux main) specified like #370, with a KVM test kit and second opinions on 26
> decisions; a running ext4 disk must have its journal replayed to be read (measured), and B's
> Linux computer is work A needs later, so the first build can start the same either way (#3xx).

## For polarzero to rule

- **Which main first**, with the question that decides it: does the first night need Xcode or macOS
  apps in main? Recommended: keep A, and build steps 1, B's Linux computer and step 6 first, which
  leaves the choice open until the Mac kit's tests 00, 01, 07 and 21 have run
  ([a-and-b.md](a-and-b.md)).
- **`checkpoint-per-system`'s first sentence**, which excludes #365's best checkpoint under both
  options ([decisions.md](decisions.md)).
- **`file-changes-from-outside`**: replay a journal before reading, a sentence to add under B.
- **`processes-from-inside-linux`**: whether the kernel's own report of exec and exit comes before
  the probe is relied on, under the direction of 20 November.
