# Which VM engine runs Linux computers

The answer is the owner's view, given on 19 November 2026, and not yet taken. The owner also said
it is decided when the time comes. In the owner's words: everything is open source, and patches
are cheap to write with agents; what Silo would add is mostly convenience: snapshots, the screen,
smoother use.

The research, with its sources and dates:
[research/linux-vm-engine.md](../../../research/linux-vm-engine.md). The facts below are as of
19 November 2026.

## Why libkrun rather than MicroSandbox

- libkrun runs on a Mac and on a Linux host, which `engine-on-linux` needs. Apple's framework runs
  only on a Mac, and Firecracker and Cloud Hypervisor only on Linux, so each would mean a second
  Linux engine.
- MicroSandbox is built on libkrun. What it adds on top (the network stack and its rules, secrets,
  images, checkpoints) is what Silo's design builds itself. What Silo needs is the VM library
  underneath.
- libkrun takes Silo's network stack through a socket, returns freed memory on a Mac since
  1.19.8, runs a VM in the process that calls it (the helper of `processes`), and has a C
  interface for the Zig engine. MicroSandbox does none of the four.
- libkrun is led by Red Hat with several regular maintainers. MicroSandbox rests on two founders
  and one new committer, and its own copy of libkrun is 653 commits behind upstream.
- Most of what broke in MicroSandbox for the earlier code of Silo was in its snapshot store and
  disk layers, which Silo would replace.

## What libkrun lacks

- Memory snapshots and forks of a running computer. MicroSandbox has them working, on a Mac and on
  Linux: this is the strongest point in its favour. Upstream has one pull request, for the Mac
  only, wanted by the maintainers and put off until after libkrun 2.0. 2.0 is not out and has no
  date, the pull request has had no commit since 15 July, copy-on-write restore (which forking
  needs) is not written, and nothing exists for Linux hosts. Nobody has given a date; getting it
  sooner means Silo doing the work.
- A window for the screen: `linux-screen`.

## Measured before deciding

To find out:

1. A Zig helper boots the Ubuntu image on libkrun through its C interface, with its traffic sent
   to a stub network stack over a socket, on a Mac and on the Linux test machine.
2. How long a fork takes from a cloned disk with a cold boot.
3. Whether systemd and Docker run in that image.
4. How much memory an idle computer gives back on a Mac.
5. How fast the work folders are, against the budget of `speed-budgets`.
