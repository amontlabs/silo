# Option B's decisions: second opinions, to rule in one sitting

An agent's second opinion, 25 November 2026, for polarzero. Option B's first build needs 96 open
decisions, counted below. #352 gave a second opinion on 56 of them and #373 on 19. This page gives
one on the rest, and on the entries whose answer has to change, or gains something, when main is a
Linux computer. Each answer is read from `plan show <id>` on #348's tree (`b45be1c`), which holds
#348's new entries. Nothing under `plan/` changes here, apart from agents' notes, and no verdict is
written.

## How the 96 were counted

[tools/count.py](tools/count.py) follows `needs` from a set of features through #348's tree and
counts the decisions with no verdict, as #373 did. It gives 87 for A (#373 said 88) and 96 for B
(#373 said 94). The difference with #373 is in how B's stand-in feature for a Linux main is made:

- #373 drops `mac-silo-needs`, `files-through-a-channel`, `main-on-macos-first` and
  `first-run-time-macos` from B. Only `first-run-time-macos` is about macOS alone.
  `files-through-a-channel` is about any main ("files go in or out only through a channel you
  grant"). `mac-silo-needs` still has to say which Mac B needs, with another answer.
  `main-on-macos-first` is the question B answers the other way, and `main-on-linux` needs it.
- Here the stand-in, `computers-linux-main-on-a-mac` (this PR adds it to the plan), needs what
  `computers-main-on-a-mac` needs apart from `first-run-time-macos` and `macos-vm-driver`, plus
  what `computers-linux-beside-main` needs apart from the macOS main and the recipe, plus
  `main-on-linux`.

| | A | B |
|---|---|---|
| Features | 19 | 20 (`processes-in-the-record` added; its need `processes-from-inside-macos` cut, as #373 did) |
| Open decisions | 87 | 96 |
| Only in A | `first-run-time-macos` | |
| Only in B | | `linux-vm-engine`, `work-on-top-of-libkrun`, `libkrun-patches`, `engine-patch-review`, `main-on-linux`, `blank-computer`, `x86-linux-computers`, `containers-on-macos`, `storage-between-computers`, `processes-from-inside-linux` |
| The same id, another answer under B | | `main-on-macos-first`, `mac-silo-needs`, `main-set-up-for-agents` |

## The sheet

| id | Recommendation under B | Confidence | In one line |
|---|---|---|---|
| `main-on-macos-first` | Under B: replace the answer | high | B is this decision answered "Linux first"; the proposed text keeps `linux-never-copies-macos-main` |
| `main-on-linux` | Under B: take with an edit | high | Its last sentence says Silo starts with a macOS main; under B it starts with a Linux one |
| `mac-silo-needs` | Under B: take with another answer | medium | Apple silicon and macOS 14 or later, to confirm with test L1; no macOS 27 |
| `linux-vm-engine` | Take as written | high | Each of its five measurements is a kit test, and the libkrun facts it rests on hold on 25 November |
| `work-on-top-of-libkrun` | Take with an edit | high | Add the write log: Silo sees main's writes in libkrun's block device, which is what makes cheap checkpoints and exact file facts |
| `libkrun-patches` | Take as written | high | The write log is the first patch, and it is shaped for upstream |
| `engine-patch-review` | Take as written | high | The write log's first input is a range main controls; the fuzz target is specified |
| `blank-computer` | Take as written | medium | B's main is not blank: `main-set-up-for-agents` installs the list, as under A |
| `x86-linux-computers` | Take as written | high | Nothing in B's first build runs x86 |
| `containers-on-macos` | Take as written | high | Under B containers run in main itself, which is what the answer allows of a Linux computer |
| `storage-between-computers` | Take as written | high | One computer in the first build; a fresh disk's unwritten blocks read as zero (K13) |
| `processes-from-inside-linux` | Take with an edit | medium | Add the kernel's own report of exec and exit as the next step, under polarzero's direction of 20 November |
| `computer-image` | Take whole, with #352's macOS edit and one Linux edit | high | B needs the Linux half #352 left open; it holds, with two hashes and a conversion |
| `checkpoint-per-system` | Take with an edit | high | Allow a checkpoint made of the blocks copied before they are overwritten, which #365 measured best |
| `file-changes-from-outside` | Take #352's text with one sentence added | high | ext4 must have its journal replayed, or a checkpoint taken within two minutes of a change misses it |
| `fork-shares-disk-blocks` | Take as written | high | Nothing changes under B on a Mac: APFS clones |
| `network-stack` | Take as written | high | Under B gvproxy with libkrun is Podman's own path, so the Mac unknown of A (test 07) does not arise |
| `network-before-rules` | Take as written | high | Under B a closed main keeps SSH from the first build, since the way in is vsock |
| `main-set-up-for-agents` | Under B: take with another list | high | No Command Line Tools or Homebrew; git, tmux, build-essential, Node.js, the agent programs |
| `processes` | Take as written | high | Under B the helper is one Zig process that holds main, the boundary and the way in |
| `which-process-holds-the-boundary` | Take as written | medium | Under B "the helper" can be read literally; the cost is that a boundary crash stops main |
| `when-the-engine-stops` | Take as written | high | Nothing about it depends on main's system |
| `where-rules-run` | Take as written | high | No rule in the first build; it binds nothing yet |
| `thinning-checkpoints` | Take #364's combined text | high | #370 already builds it; nothing changes under B |
| `kinds-of-other-actions` | Take as written | high | Put-back and checkpoint removal are destructive under both options |
| `record-queried-with-ordinary-tools` | Take as written | high | `silo --json` is the way, as #370 specifies |

Twenty-six rows. Sixteen are taken as written, four with an edit, three with another answer under
B, and three with another draft's text (#352's or #364's), two of them with a sentence added. None fits what `owners-part-on-engine` lets agents
take alone: each promises something you see, or sets a word or a property, or polarzero has
written in its notes. `libkrun-patches` and `engine-patch-review` come closest, being about how
Silo is built, and are left to polarzero because `linux-vm-engine`'s notes carry polarzero's words.

---

## `main-on-macos-first`

**Asks:** Which system does main run first?

- **Recommendation under B:** replace the answer. Under A, #352's "take as written" stands.
- **The case:** polarzero runs Claude Code and Codex in main overnight on a Mac that runs macOS 26.5.
  Under A the first step is updating the Mac to macOS 27 and downloading 25.6 GB. Under B main is
  Ubuntu, made in minutes, with Docker in it, and the agents' work on web and server code runs as it
  does on Linux CI. What polarzero gives up is Xcode and macOS apps in main.
- **Proposed text** (answer; the property it sets stays, since it is about forks):

  > Main is a Linux computer first. Its forks are Linux. A macOS computer comes later, as a
  > computer of its own beside main.

  `sets` keeps `linux-never-copies-macos-main` unchanged: no Linux computer is ever a copy of a
  macOS main, under either answer.
- **What decides it:** the side-by-side in [a-and-b.md](a-and-b.md).

## `main-on-linux`

**Asks:** Can main be a Linux computer?

- **Recommendation under B:** take with an edit.
- **The case:** the answer's last sentence, "Silo starts with a macOS main on a Mac", is the one
  thing B changes.
- **Proposed text:**

  > Yes. On a Mac, main is a Linux computer or a macOS computer, and you choose when you make it.
  > On a Linux host, main is a Linux computer, since macOS runs only on Apple hardware. Silo
  > starts with a Linux main on a Mac.

- Its place in `order.json` moves from the stage "Later" to the first build's block, as
  `computers-linux-main-on-a-mac` needs it.

## `mac-silo-needs` (#348)

**Asks:** Which Mac does Silo need?

- **Recommendation under B:** take with another answer.
- **The case:** under A the answer is "macOS 27 or later" because only macOS 27 provisions a new
  macOS computer with nobody clicking. Under B nothing needs provisioning: cloud-init sets up
  Ubuntu. libkrun's own floor is what Podman's libkrun machines state, macOS 14, which test L1
  confirms.
- **Proposed text:**

  > A Mac with Apple silicon that runs macOS 14 or later. On an Intel Mac, the first run says so
  > and starts nothing.

- **Needs:** `computer-image` only; `main-on-macos-first` no longer applies.
- **What only a Mac can answer:** test L1 on the oldest macOS the helper boots on.

## `linux-vm-engine`

**Asks:** Which virtual machine monitor runs Linux computers, on a Mac and on a Linux host?

- **Recommendation:** take as written.
- **Confidence:** high, for the facts. Re-read on 25 November 2026: libkrun's `main` has the 2.0
  interface (pause and resume, an orderly stop on a Mac, a balloon with free page reporting, an
  external kernel), 2.0 is still not tagged, and the newest release is 1.19.8. #366 compared it on
  a Linux host with Firecracker, Cloud Hypervisor and QEMU and found nothing that changes the
  choice for the first build.
- **The case:** under B, main itself runs on libkrun. Its five measurements are the kit's K01
  (boot), K06 (a cold fork), K02 (systemd and Docker), L2 (memory given back) and L3 (disk
  speed), each with an outcome that says whether the choice holds.
- **One note to add:** the first build's helper is written against the 2.0 interface pinned to a
  commit, because 1.19 has no pause, which `pause-every-computer` needs. The kit uses 1.19, which
  can be installed today. Kernel format constants differ between the two (`KRUN_KERNEL_FORMAT_RAW`
  is 0 in 1.19 and 1 in 2.0); the helper names them, never their numbers.

## `work-on-top-of-libkrun`

**Asks:** What does Silo build on top of libkrun?

- **Recommendation:** take with an edit.
- **The case:** #365 measured checkpoints by clone making each write of the computer wait 9 to
  12 s once its disk image is in half a million pieces, and checkpoints by copying the blocks about
  to be overwritten keeping writes under 160 ms. The second needs Silo to see each write. Under
  libkrun every write passes through one function of its block device (`process_request` in
  `src/devices/src/virtio/block/worker.rs`), in the helper. A hook there gives the write log
  ([4-the-way-back.md](4-the-way-back.md)), and the same log tells step 5's comparer exactly which
  blocks changed.
- **Proposed text** (one sentence added after "…made as `fork-shares-disk-blocks` says for each
  host filesystem, ext4 included;"):

  > a log of the blocks each computer writes, kept in the helper from a hook in libkrun's block
  > device, from which checkpoints copy only what is about to be overwritten and the record learns
  > which blocks changed;

## `libkrun-patches`

**Asks:** How does Silo carry its patches to libkrun?

- **Recommendation:** take as written.
- **The case:** the write log is the first patch the first build needs: one setter and a hook in
  one function. It is shaped as a general feature (QEMU's dirty bitmaps serve the same purpose for
  incremental backups), which gives it a chance upstream. If refused, the answer already says what
  happens.

## `engine-patch-review`

**Asks:** How is a patch to libkrun reviewed?

- **Recommendation:** take as written.
- **The case:** the write log's hook receives a sector and a length that main chose. Reading
  libkrun's `main` on 25 November, the read path computes its offset with `checked_mul` and the
  write, discard and write-zeroes paths multiply `sector * 512` unchecked. Whether the disk layer
  below refuses a wrapped offset was not checked. That is the review the answer asks for: the
  patch brings a fuzz target, and the target answers it.

## `blank-computer`

**Asks:** What is in a computer that no recipe set up?

- **Recommendation:** take as written.
- **The case:** under B, Ubuntu with Silo's SSH unit and the probe is the blank computer. Main is
  not blank: `main-set-up-for-agents` installs the list after your yes, as under A. The answer
  needs no change.

## `x86-linux-computers`

**Asks:** Where does an x86 Linux computer run?

- **Recommendation:** take as written.
- **The case:** an agent in main runs `docker run postgres:17`. The image is multi-architecture and
  arm64 is pulled. An image published for amd64 only fails, or runs under QEMU's emulator if the
  agent installs `binfmt` handlers, several times slower. That is what the answer says, and Silo
  adds nothing for it in the first build.

## `containers-on-macos`

**Asks:** Can a macOS computer run containers?

- **Recommendation:** take as written.
- **The case:** under A an agent in main that needs `docker compose up` for its tests cannot run
  it: there is no Linux computer beside main in the first build. Under B Docker runs in main
  itself (K02). The answer already says containers run in a Linux computer.

## `storage-between-computers`

**Asks:** Can a computer read what another computer left on disk or in memory?

- **Recommendation:** take as written. It sets the property `storage-never-crosses-computers`.
- **The case:** main's disk is a sparse file; a block main never wrote reads as zero from the
  file system's hole. K13 reads the tail of main's disk as root and counts non-zero bytes. A
  restored main reads its checkpoint, which is its own past.

## `processes-from-inside-linux`

**Asks:** Which means give the processes of a Linux computer from inside?

- **Recommendation:** take with an edit.
- **Confidence:** medium. Nothing of the probe has run on Silo's kernel; K15 measures its means.
- **The case:** at 02:40 Codex runs `sudo systemctl stop silo-probe`, then `rm -rf` a folder
  outside the repository. The record shows the probe went silent, then the file facts of the next
  checkpoint, with no process behind them. Under polarzero's direction of 20 November ("as many
  facts as possible, which cannot be tampered with"), there is a way root cannot stop: Silo's
  kernel, which main cannot replace or extend (no modules, no kexec, no `/dev/mem`), reports each
  exec and exit itself through a hypervisor call that the helper receives (#351, section 3.1).
- **Proposed text** (one sentence added at the end):

  > Before the probe is relied on, a report of each exec and exit made by Silo's own kernel through
  > a call to the helper is measured against the same target; it cannot be stopped by root inside
  > without a kernel exploit, and where it meets the target it replaces the probe for starts and
  > exits.

- **Why the first build keeps the probe:** the kernel report is a patch to carry in the kernel and
  in libkrun, with no trial behind it. The probe is the answer as written and can be built with
  what exists.

## `computer-image`

**Asks:** Where does a computer's system image come from?

- **Recommendation:** take whole: #352's macOS edit, and the Linux half with one edit.
- **The case:** #352 proposed taking the macOS half and leaving the Linux half open, as the first
  build did not need it. Under B it does. Ubuntu's 26.04 image for arm64 is a qcow2 file of 943 MB
  in which 95 % of the clusters are compressed (read on 25 November, [proto/results.txt](proto/results.txt)).
  Silo boots a raw disk, so it converts it once, and the copy it keeps is the raw disk.
- **Proposed text** (after "…checks it against Ubuntu's published checksum and keeps it"):

  > converted to a raw disk whose checksum Silo also pins,

## `checkpoint-per-system`

**Asks:** What is a checkpoint made of on each system?

- **Recommendation:** take with an edit.
- **The case:** the answer says a checkpoint is "a copy of the computer's disk that shares blocks
  with it". #365's best way, `delta`, is a file of the blocks copied just before main overwrites
  them: it shares nothing with the live disk, and nothing is cloned while main runs, which is why
  writes do not wait. Under the answer as written, `delta` is not a checkpoint. It is open; A and B
  both benefit, B first.
- **Proposed text** (replace the first sentence):

  > On every system, a checkpoint is a copy of the computer's disk at one moment, taken from
  > outside the computer: either a copy that shares blocks with the disk, by the same means as a
  > fork's disk (`fork-shares-disk-blocks`), or the blocks the computer overwrites after that
  > moment, copied just before each write, where Silo sees the computer's writes.

## `file-changes-from-outside`

**Asks:** Which route gives Silo the file changes of a computer from outside?

- **Recommendation:** take #352's text, with one sentence added.
- **The case:** measured on 25 November ([proto/results.txt](proto/results.txt)): copies of a
  mounted ext4 disk taken 7 s and 30 s after a burst of changes showed none of them to a reader that
  ignores the journal; at 120 s they all showed. With checkpoints every 5 minutes, that misplaces
  the last minutes of work before each checkpoint. With the journal replayed in memory, the trial
  reader matched the kernel exactly, and on Ubuntu's own image it listed the same 107,853 paths.
- **Proposed text** (added after the sentence on Silo's own reader):

  > On a filesystem with a journal, such as ext4, the reader replays the journal in memory before it
  > reads, as the computer's kernel does at its next boot, and never writes it back.

## `fork-shares-disk-blocks`, `network-stack`, `network-before-rules`

Take as written, under B as under A (#352 and #373's opinions stand). Under B:

- forks are not in the first build, and a checkpoint of `disk.raw` on a Mac is an APFS clone as
  the answer says;
- gvproxy with a libkrun computer is the path Podman machine uses on a Mac, so #359's test 07,
  which #373 makes blocking for A, does not arise;
- main's way in is vsock, so "closed" in `network-before-rules` keeps SSH from the first build,
  which #370 could offer only once gvproxy replaced Apple's NAT.

## `main-set-up-for-agents` (#348)

**Asks:** What is in main when the first run ends?

- **Recommendation under B:** take with another list.
- **Proposed text** (the list):

  > …each vendor's own installer, after it shows you the list: git, tmux and the build tools from
  > Ubuntu's archive, Node.js, and the agent programs you choose among those Silo offers.

## `processes`, `which-process-holds-the-boundary`, `when-the-engine-stops`, `where-rules-run`

Neither #352 nor #373 gave these a second opinion; #348 lists them for `engine-outlived-by-computers`.
Take all four as written, under both options.

- **`processes`.** Under B the helper is one Zig process: libkrun runs main in it, a second thread
  serves the engine's socket, and gvproxy is its child. "Only a helper takes something with it:
  its own computer" holds exactly.
- **`which-process-holds-the-boundary`.** Under B the boundary is a library of the helper, which is
  the answer read literally. The case against it: one of #358's malformed requests that crashes
  the boundary also stops main. [6-model-traffic.md](6-model-traffic.md) adds a test that none
  does. Confidence medium, since the alternative (a second process in the helper's job, #373's
  reading) costs little and keeps main running through a boundary crash.
- **`when-the-engine-stops`** and **`where-rules-run`** do not depend on main's system. No rule
  exists in the first build, so `where-rules-run` binds nothing yet.

## `thinning-checkpoints`, `kinds-of-other-actions`, `record-queried-with-ordinary-tools`

- **`thinning-checkpoints`:** take the text #364 combines (every automatic checkpoint for 24 hours,
  one per hour for 2 days, one per day for 7 days, and never a save, a checkpoint you took, an
  uncompared one or the newest), which #370 builds. Nothing changes under B.
- **`kinds-of-other-actions`:** take as written. Put-back and removing a checkpoint are destructive,
  so they need you; #370 asks before each.
- **`record-queried-with-ordinary-tools`:** take as written. `silo --json` prints one line per fact
  with `"v":1`, as #370 specifies.

## What the 96 do not settle for B

- **A new feature:** `computers-linux-main-on-a-mac`, added by this PR as an undecided entry, in
  place of `computers-main-on-a-mac` for B's first build.
- **Silo's kernel** is a new thing Silo ships and keeps up: a configuration, a long-term release,
  a build in CI. No entry says so. It sits under `linux-vm-engine` and `libkrun-patches`; if
  polarzero wants it named, it is a decision "Which kernel does a Linux computer boot?" with the
  answer of [2-main-runs.md](2-main-runs.md).
- **The way in over vsock** is how `computer-reached-over-ssh` reaches a Linux computer. The entry
  does not say by which transport; nothing needs to change.
