# A and B side by side: what each needs before the first night

An agent's comparison, 25 November 2026, for polarzero. A is #373's recommended first build: a
macOS main with the record of model traffic. B is the same first build with a Linux main on the
Mac, specified in this folder. Both have the same engine, recorder, boundary and `silo` command;
they differ in the computer, its checkpoints and the reader of its disk.

"The first night" is #373's: Claude Code and Codex left in main overnight, and in the morning the
deleted folder, the false «All tests pass.», the paste site and the night's spend.

## In one table

| | A: macOS main | B: Linux main |
|---|---|---|
| What runs in main | macOS 27, Xcode possible, no Docker | Ubuntu 26.04 on arm64, Docker, no Xcode or macOS apps |
| Your Mac needs | Apple silicon, **macOS 27** (polarzero's ran 26.5 on 21 November) | Apple silicon, macOS 14 or later (to confirm, test L1) |
| First download | 25.6 GB restore image, then an install | 943 MB image, converted in seconds |
| How main is set up with nobody at the screen | Apple's guest provisioning, new in macOS 27, with one reported failure when the user name is the Mac's (FB23241619) | cloud-init from a seed disk, as Lima and Multipass do |
| The way in | SSH over the network, through gvproxy once it works with a macOS computer | SSH over vsock: works with main's network closed |
| Silo's authority for model traffic | main's System keychain over SSH if test 21 allows it, otherwise each program's setting | Linux's store and the same settings, as #347 ran |
| Checkpoints | APFS clone; a `delta` that keeps writes short needs main served over NBD, which no one has shown a macOS computer boots from (test 32) | APFS clone first; `delta` through a hook in libkrun's block device, which Silo builds |
| Which blocks changed between checkpoints | #359 test 05 (`F_LOG2PHYS_EXT`) | The write log: exact, and a fact |
| The disk reader | APFS: strict, with `decmpfs` (zlib, LZVN, LZFSE) for contents; no trial has read contents | ext4 with its journal replayed: a trial matched the kernel on live copies and on Ubuntu's image |
| Processes in the record | not known yet (needs a probe in macOS, held back by `macos-ships-facts-first`) | yes, from the probe, as reports (step 7) |
| The kernel | Apple's | Silo's own build, which main cannot replace or extend |
| Apple's limit of two macOS computers | applies: one fork at most beside main | does not apply |
| Features / open decisions (counted on #348's tree) | 19 / 87 | 20 / 96 |
| Specs | #370 steps 1 to 5, #373's changes and step 6 | #370 step 1, this folder's steps 2 to 7, #373's step 6 |
| Can be tested before the Mac | the boundary and the record, against a Linux test computer | everything but L1 to L8, on a Linux host with KVM (the kit, K00 to K16) |

## Before the first night, in order

### A

1. Update the Mac to macOS 27.
2. Mac kit, blocking: tests 00, 01, 10, 14 (download, install, provisioning with your own user name,
   a first start cut off, what provisioning leaves), 07 (gvproxy under a macOS computer, now
   before main's first start), 21 (the authority in the Keychain over SSH), 04 (a clone of a
   running disk is a moment in time), 09, 06, 20.
3. Build #370's step 1, #373's step 6 against a Linux test computer, then #370's step 2 with the
   boundary in front from the first start.
4. Mac kit for steps 4 and 5: tests 03, 15, 17, 31, 32, 33; 05, 16, 18, 19.
5. Build steps 3, 4 and 5 (APFS reader for metadata; contents later with `look`).
6. Test 34 in main: a whole night through the boundary.

Unknowns that can stop A before the first night: provisioning (01), gvproxy with a macOS computer
(07), the Keychain over SSH (21; a fallback exists), a clone of a running disk (04; a fallback
exists, a pause), the APFS reader on a real main (17, 19).

### B

1. A Linux host with KVM on arm64, for the kit. No change to the Mac.
2. Kit K00 to K07 and K13 (boot, Docker, SSH over vsock, network, stop, fork, clone moment, what
   root can do). Mac tests L1, L2, L5, and #359's 06, 09, 20.
3. Build #370's step 1, then step 2 under B, then #373's step 6 against main itself.
4. Kit K08 to K12, Mac tests 03, 05, 31, 33, L8. Build steps 3, 4 (clone first) and 5.
5. Step 7, the probe, with K15.
6. K14 on the Linux host, then the same night on the Mac.

Unknowns that can stop B before the first night: Silo's kernel booting Ubuntu with systemd
`running` and no modules (K01), the helper on HVF on polarzero's macOS (L1), a clone of `disk.raw`
on APFS being a moment in time (L5; a fallback exists, a pause). libkrun 2.0 is not tagged, so the
helper pins a commit of `main`.

## What the first morning shows

#373's table, with B's column filled from this folder's specs:

| In the morning, you see | A | B |
|---|---|---|
| The deleted folder, and the checkpoint before it | yes | yes |
| Each command the agent ran, with its exit code as sent to the model | yes, a report read from the traffic | yes, and the exit the probe saw beside it |
| «All tests pass.» beside the suite's exit 1 | yes | yes, seen twice |
| The paste site reached, and how much was sent | yes | yes |
| What each conversation spent, and the plan's share | yes | yes |
| A process left running after the run ended | no | yes, a report |
| Docker containers the agents started | no Docker in main | yes, as processes |
| Mac or iOS work in main, with Xcode | yes | no |

## What carries over

- **From A to B, if the Mac kit fails** (#373's fallback): the engine, the recorder, the boundary,
  step 3 and step 6 carry over. Steps 2, 4 and 5 are this folder's.
- **From B to a macOS main later:** the same pieces carry over, and steps 2, 4 and 5 for macOS are
  #370's. #373 counts this as writing those steps twice. It is not quite that: the plan needs a
  Linux computer anyway (`computers-linux-beside-main`), with a libkrun helper, Linux checkpoints
  and a reader of ext4. Under A that work comes in the third build; under B it comes first. What B
  writes for main is what A writes later for the computer beside it.
- **Your main itself** does not carry over between systems: a macOS main is never a copy of a Linux
  one (`linux-never-copies-macos-main` and its mirror). Moving from a Linux main to a macOS main is
  a new main, with your repositories brought in again.

## Which to take

polarzero leaned to macOS first on 18 November ("much more interest this way"), and #373
recommends A on the same ground: a Mac of its own is what people give their agents, and no cloud
offers it. This comparison does not overturn that. It adds three things the earlier notes did not
weigh:

1. **B's work is not thrown away under A.** It is the Linux computer the plan needs anyway, done
   first.
2. **A has more ways to fail before the first night**: macOS 27 on the Mac, provisioning, gvproxy
   with a macOS computer and the Keychain over SSH, against B's kernel boot and the helper on HVF.
   More of B can be checked before the Mac, on a Linux host with KVM.
3. **B's first morning is fuller**: processes, the exit the probe saw beside the one sent to the
   model, and Docker in main.

**Recommended: keep A as the first build's main, and build in an order that leaves the choice
open until the Mac has answered.**

1. #370's step 1.
2. B's step 2 as a Linux computer, not yet main: it is #373's "Linux test computer" for the
   boundary, and the start of `computers-linux-beside-main`.
3. #373's step 6 against it.
4. Then the choice, with the Mac kit's tests 00, 01, 07 and 21 in hand. If they pass, A's step 2
   (#370 with #373's change) makes the macOS main. If one fails without a fallback, the Linux
   computer of step 2 becomes main, and steps 3 to 7 follow this folder.

Nothing in steps 1 to 3 depends on the answer to `main-on-macos-first`. The one question that
should decide it, if polarzero wants to decide now: **does the first night need Xcode or macOS
apps in main?** If not, B is the first night with fewer unknowns.
