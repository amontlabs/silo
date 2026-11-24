# The smallest first build of Silo

An agent's proposal, 24 November 2026, for polarzero to review. It changes nothing in `plan/`: no
verdict, no answer and no order is edited. Every id below is an entry of the plan, and
`plan show <id>` gives it. The counts come from reading `plan/` at `1b8f1917a` with a script, and
can be checked the same way.

## In short

Build first the part of Silo that lets you run Claude Code or Codex with every check off on your
Mac and lose nothing: main as a macOS virtual machine, reached over SSH, with checkpoints taken on
their own, restore, and a record of which files changed between two checkpoints. It is read from
the `silo` command. There is no app, no rules, no model traffic and no Linux yet.

That is 12 features. Before they can be built, 56 open decisions need a verdict. The other 250 open
decisions can wait. About 14 of the 56 are about how Silo is built, and agents could take them with
a judge, as `owners-part-on-engine` allows. That leaves about 42 for polarzero. Eight things can
only be measured on a Mac, and two risks are not in the plan at all: getting a fresh macOS
computer past Setup Assistant with nobody clicking, and reading a disk that a hostile agent wrote
without mounting it in your Mac's kernel.

## Where the plan stands

| | Count |
|---|---|
| Entries | 626 |
| Decisions | 492: 160 taken by polarzero, 23 by agents, 3 dropped, 306 with no verdict |
| Features | 134, none with a verdict |
| Features ready to build | 0 |

Every feature needs at least one decision with no verdict, except one. `access-model-checked` needs
only taken decisions, so it is the one feature polarzero could take today and an agent could start
at once.

The owner is shown the open decisions in the order of the stage `ground`. The next one is
`word-agent`. The 56 decisions the first build needs sit among the first 157 of the 293 open
decisions of that stage, so in the current order about 100 decisions that the first build does not
need come before its last one. Most of those are about words, principles and later parts, such as
`bad-paths-principle`, `escalation-question` and the boundary's holds.

## The first build

### What you do with it

You make main once. Silo downloads Apple's restore image, installs macOS in a virtual machine, and
writes an SSH entry for it. You run `ssh main`, clone two repositories, and start Claude Code with
`--dangerously-skip-permissions` in one and Codex with its sandbox off in the other. You go to bed.

In the morning, one of them has deleted a folder it should not have touched and broken Homebrew.
`silo checkpoints main` lists the checkpoints taken every few minutes. `silo changes main --since
<checkpoint>` prints, as one JSON line per file, what was created, changed and deleted between two
checkpoints. You run `silo restore main <checkpoint>`. Silo first saves where you were, then puts
the whole disk back, conversations included, and you take Claude Code's conversation up again with
`claude --resume`.

That case is the promise of `the-shape` without any of the parts that make Silo large. The tools
polarzero has today each cover part of it: Time Machine backs up files once an hour, `git` sees only
the repositories, and Tart or UTM run a macOS virtual machine but take no checkpoints on their own
and keep no record of what changed.

### The features

In the order they would be built:

| Feature | Why it is in the first build |
|---|---|
| `access-model-checked` | Its needs are all taken, `model-of-access-first` asks for it first, and it is small |
| `engine-program-and-clients` | Everything talks to the engine |
| `engine-matches-wanted` | main has to come back after the Mac restarts |
| `computers-main-on-a-mac` | The computer itself |
| `agent-programs-run-unmodified` | You install the agent programs the vendors' way |
| `agent-programs-reach-every-computer` | `ssh main` is the way in, and it is how every editor and agent app reaches main |
| `computers-stopped-and-paused` | `restore-to-a-checkpoint` needs it |
| `checkpoints-taken-on-their-own` | The way back |
| `restore-to-a-checkpoint` | The way back |
| `record-of-a-computer` | Needed by every line of the record |
| `file-changes-in-the-record` | The first lines worth reading, and they come from comparing checkpoints, which the first build already takes |
| `files-imported-from-your-mac` | Nothing is shared from your Mac (`device-not-shared`), so this is how a file you have gets in |

The command line is the only place to see any of it. No feature of the plan says that yet:
`app-as-text` needs `now-in-the-app`, so in the current plan the command line comes after the app.
The decision `record-queried-with-ordinary-tools` already says the `silo` command prints the record
as JSON lines. A new feature, written for the first build, would say that every feature above is
reached from the `silo` command from the day it is built, and that the app's places come later and
read the same thing.

### What is left out, and why

| Left out | Where it comes |
|---|---|
| Forks, the bin, a fork becoming main (`forking-a-computer`, `dropping-into-the-bin`, `fork-replacing-main`) | Second build. A fork needs `fork-identity`, `fork-and-sign-in` and `what-a-fork-is-given`, and restore already gives the way back |
| Rules, holds, secrets, open or closed network (`allow-hold-or-deny`, `open-limited-or-closed`, `secrets-held-outside`) | Second and third builds. `open-limited-or-closed` alone pulls in 56 open decisions, because it needs the whole rule engine |
| Model traffic and spend (`model-traffic-read`, `spend-per-conversation`) | Second build at the earliest: 77 open decisions, TLS on a macOS computer is not tried yet, and `terms-on-changing-a-reply` is open |
| Linux beside main, recipes (`computers-linux-beside-main`, `made-from-a-recipe`) | Third build. Main on macOS does not need them |
| Processes and runs (`processes-in-the-record`, `what-each-run-wrote`) | Second build. They need the probe inside macOS, which `macos-ships-facts-first` holds back |
| Opinions, classifiers, extensions, views, receipts, tasks | After the record has a few kinds of lines to judge |
| The simulator of the whole core (`simulated-core-histories`) | Built beside the first build, not before it. See the challenges below |

## What has to be decided first

### Three needs to cut

Without these cuts, the first build needs 64 open decisions and pulls in recipes. Each cut is an
edit of one `needs` list.

| Feature | Need to remove | Why |
|---|---|---|
| `agent-programs-run-unmodified` | `agent-program-data-file`, `offered-programs-list` | A program you install yourself runs without a data file, as `unmodified-agent-programs` already says. The data file needs `recipe-pieces` and `recipe-audit`, which drag in the recipe part. The features that read the data file, such as `agent-programs-logins-kept-out-of-copies`, should need it instead |
| `restore-to-a-checkpoint` | `conversation-taken-up-again` | A restore puts the disk back whether or not a program takes up its conversation. The decision already says Silo claims it only for the programs it has checked. It needs `conversation-resumes-in-a-copy`, which needs `fork-and-sign-in`, a fork decision |

`conversation-resumes-in-a-copy` could also be split: the case of a restore on the same computer
needs no new sign-in, while the case of a fork does.

### The 56 decisions

By part. "Find out" means the answer says something is still to measure or to try; taking it does
not end that work, but lets the build start on the rest.

**Could be taken by agents with a judge** (sets nothing, about how Silo is built, no word of
polarzero's in its notes): `core-dependencies`, `core-knows-no-agents`, `core-state-stays-small`,
`engine-inputs`, `engine-state`, `engine-source-checks`, `engine-saved-state` (find out),
`events-until-the-recorder` (find out), `record-speed-cost` (find out), `sources-number-their-lines`,
`tigerstyle-by-place`, `own-cost-targets`. Two more set nothing but hold polarzero's own words, so
polarzero takes them: `how-the-engine-is-built`, `measuring-the-store`.

**For polarzero**, grouped so that one sitting covers a part:

- The engine's shape and its promises: `engine-shape` (sets six words), `deterministic-core`,
  `engine-limits`, `engine-reconciles`, `how-effects-work`, `numbered-events`, `call-ids`,
  `formats-carry-versions`, `who-can-call-the-engine` (find out), `network-stack` (find out).
- Main: `main-on-macos-first`, `one-main-or-several`, `neutral-core`, `computer-image`,
  `first-run-time-macos`, `files-through-a-channel`, `pause-every-computer`, `what-stop-ends`.
- Reaching main and the agent programs: `computer-reached-over-ssh`, `reaching-a-computer`,
  `documented-surfaces`, `unmodified-agent-programs`.
- Going back: `checkpoint-per-system` (find out), `fork-shares-disk-blocks` (find out),
  `checkpoint-storage`, `restore-saves-first`, `what-a-restore-leaves`, `why-destructive`,
  `meanings-for-computers` (sets ten words).
- The record: `word-line`, `word-gap`, `sources`, `kinds-of-lines`, `who-made-each-change`,
  `changes-to-the-record`, `enforcing-and-recording`, `observing-and-waiting`,
  `record-path-and-core`, `git-history-inside`, `file-changes-from-outside` (find out),
  `record-overhead-limit`.
- The product: `silo-is-light`.

`what-stop-ends` and `fork-shares-disk-blocks` carry a "find out" only for Linux, so on a Mac they
can be taken as they are.

### Build order

Recommended edits to `plan/order.json`, none made here:

1. In the stage `ground`, move the 56 decisions above to the front, in the order of the groups
   above. polarzero then rules on what the first build needs before anything else.
2. In the stage `app`, move the 12 features above to the front, in the order of the table, and the
   new command-line feature right after `engine-program-and-clients`.
3. Move `simulated-core-histories` after `computers-main-on-a-mac`. The core simulator itself
   (`core-simulator`, taken) is built with the engine from the first day; what moves is the feature
   that checks every property of the whole product.
4. Move `what-to-say`, `presenting-it` and `objections` after the second build. They are about the
   public, and the first user is polarzero (`who-it-is-for`).
5. Move `matching-reports-to-facts`, `precaution-on-unsure`, `how-a-hold-ends`,
   `reason-in-a-refusal`, `denied-tool-call-mechanism`, `denial-per-program` and
   `terms-on-changing-a-reply` next to the features that need them. They sit at the top of `app`
   today, ahead of the engine.

## What only a Mac can answer

The trials so far ran on Linux, in a container with no KVM (draft PR #347). These need polarzero's
Mac, or any Apple silicon Mac. Each one unblocks an entry of the first build.

1. **APFS clone of a running disk image** (`checkpoint-per-system`, `fork-shares-disk-blocks`).
   PR #347 found that on XFS a reflink copy of a disk in about 500,000 pieces stopped the
   computer's writes for 11 to 23 seconds, and that the checkpoints themselves cause the pieces.
   Checkpoints taken every few minutes would hit this within days. On a Mac: `cp -c` of a raw image
   and of an ASIF image (macOS 26), after 10, 100 and 1,000 checkpoints of a disk under writes,
   with the time of the clone and the longest stall of a write inside the computer. Also whether
   the clone is a moment in time while the virtual machine runs with `VZDiskImageStorageDeviceAttachment`
   in each caching and synchronization mode, or needs a pause of the virtual machine.
2. **A fresh macOS computer with nobody clicking** (`computer-image`, `first-run-time-macos`). Not in
   the plan. After Apple's installer runs, macOS starts on Setup Assistant, which asks for a user, a
   region and more, and SSH is off. Tart's images get past it with Packer typing keystrokes over
   VNC, timed per screen, which breaks when Apple changes a screen. The other way to try: before
   the first start, write the user, the flag that Setup Assistant is done and the SSH service onto
   the new disk from outside. Nothing inside has run yet, so that disk is not hostile.
3. **Reading the files of a macOS computer from outside** (`file-changes-from-outside`). Not in
   the plan as a risk. A macOS computer's disk is APFS. Mounting a clone of it on your Mac hands a
   filesystem that an agent with `sudo` wrote to your Mac's kernel, and a crafted one is a kernel
   bug away from your Mac. A reader in user space, such as libyal's `libfsapfs`, read-only and
   fuzzed as `fuzz-untrusted-parsers` already asks, keeps it out of the kernel. The test: walk two
   checkpoints with it and compare, on a disk with a few million files, and time it. Also what
   happens when an agent turns FileVault on inside: the reader then sees nothing, which must show
   as a gap, not as "nothing changed".
4. **What a forced write costs** (`engine-saved-state`, `events-until-the-recorder`). On a Mac,
   `fsync` does not reach the drive; `F_FULLFSYNC` does, and costs milliseconds. The engine writes
   every input through before applying it, so the cost per input decides whether writes are
   grouped.
5. **A network stack of Silo's own under a macOS computer** (`network-stack`).
   `VZFileHandleNetworkDeviceAttachment` passes raw Ethernet frames over a datagram socket, and
   gvisor-tap-vsock (gvproxy) already speaks that protocol for vfkit and Podman. Measured: the speed
   of `git clone` and `npm install` of a large project through it, against Apple's NAT. If it is
   close, `network-stack` can be taken with gvproxy as the first driver behind `core-dependencies`,
   and the rule engine added later on the same path.
6. **How the app and the command line prove who they are** (`who-can-call-the-engine`). On a Mac a
   Unix socket gives the caller's user through `getpeereid`, and an XPC connection gives an audit
   token that can be checked against Silo's code signature. Which one holds when an agent on your
   Mac, outside every computer, calls the engine, is the test.
7. **The first run's time** (`first-run-time-macos`): the download, the install and the first
   start, timed once.
8. **Trust from the macOS keychain** (`reading-each-agent-program`, second build). On Linux the
   system's store was enough for both programs. Claude Code is now a single binary and Codex is
   Rust; whether each reads the macOS keychain, or only its own list of authorities, decides how
   model traffic is read in a macOS computer.

## Challenges to decisions in force

As `AGENTS.md` asks. Each stays in force until polarzero changes it.

- **`model-of-access-first`** says the first engine code is the part that decides who can get
  what. In the first build almost nothing is granted: your Mac's way in over SSH, and files you
  import. A model of access checked on every short history is worth most when there are secrets,
  hosts and rules to grant, which is the second build. The feature is small and ready, so the cost
  of building it first is low; the risk is that its tables are shaped before any real grant exists
  and are redone when secrets arrive. A possible replacement: the access model is written first,
  as it says, but against the grants of the second build (secrets, hosts, the SSH way in), written
  down before its code.
- **`network-stack`** has no verdict, but `computers-main-on-a-mac` needs it, so main cannot start
  until Silo has a network stack of its own. With gvproxy as a driver (Mac test 5), most of that stack
  already exists. If the test shows it is slow, main could start on Apple's NAT for the first
  build, with the network marked as having no boundary, and move to the stack in the second build.
  That would need `network-stack` to say so.
- **`engine-pace`** ("slowly on the engine") has no verdict, and its notes hold polarzero's own
  words. This proposal does not ask to build faster. It asks that the slow, checked work be done on
  the pieces the first build uses, and not on the 250 decisions it does not.
- **`app-as-text`** needs `now-in-the-app`, so the command line waits for the app. For the first
  build the order is the other way: the command line is the only surface, and an agent inside a
  computer or on your Mac reads the record from it, which is what `silo-command-reads-the-app`
  (taken) is for.

## After the first build

These are sketches, to show that the first build leads somewhere and is not a dead end.

- **Second build: forks and the edge.** `forking-a-computer`, `dropping-into-the-bin`,
  `agent-programs-conversation-in-a-fork`, an open or closed network split out of
  `open-limited-or-closed` without host lists, `hosts-reached-and-denied` from the network stack,
  `processes-in-the-record` on macOS, and `secrets-held-outside`. This is where a fork with no
  secrets and a closed network lets an agent try a tool you do not trust.
- **Third build: traffic, rules and Linux.** `allow-hold-or-deny`, `held-requests-and-asks`,
  `model-traffic-read`, `model-calls-in-the-record`, `spend-per-conversation`, `made-from-a-recipe`,
  `computers-linux-beside-main`.
- **Then the app's places** (`now-in-the-app`, `timeline-in-the-app`, `what-needs-you`), once the
  record holds files, processes, hosts and model calls to show.

## Sources

- The plan at commit `1b8f1917a` of `amontlabs/silo`, read with a script (`needs` followed through
  every entry).
- Draft PR #347, "what a first run of six 'to find out' checks found on Linux".
- Tart's Packer builder, on `boot_command` typed over VNC and the ASIF disk format:
  https://developer.hashicorp.com/packer/integrations/cirruslabs/tart/v1.17.0/components/builder/tart
- Apple, `VZDiskImageStorageDeviceAttachment` and ASIF in macOS 26:
  https://developer.apple.com/documentation/virtualization/vzdiskimagestoragedeviceattachment and
  https://eclecticlight.co/2025/06/12/macos-tahoe-brings-a-new-disk-image-format/
- Apple, on `VZFileHandleNetworkDeviceAttachment` and a network stack of one's own:
  https://developer.apple.com/forums/thread/702319
- boxlite's gvproxy backend, on the vfkit protocol over Unix datagram sockets on macOS:
  https://docs.rs/crate/boxlite/0.9.5/source/src/net/gvproxy/mod.rs

Not verified here, and to check before relying on them: that `libfsapfs` reads a macOS 26 Data
volume, and how each agent program loads its authorities on macOS.
