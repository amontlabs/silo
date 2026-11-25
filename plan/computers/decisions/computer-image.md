# Where a computer's system image comes from

## The choice

An agent's part, 22 November 2026. You make your first Linux computer beside main to run your tests
on Linux. Its system has to come from somewhere. For main's macOS there is no choice: Apple forbids
shipping it installed, so each Mac downloads and installs its own, over 10 GB.

The options:

- Ubuntu, downloaded the first time (recommended). Silo fetches Ubuntu's official image, under 1 GB,
  once, and every later Linux computer starts from that copy. Agents get the system most guides use
  and GitHub's own build machines run. It costs that download the first time you make a Linux
  computer.
- Ubuntu, shipped inside Silo. Nothing to wait for later. It costs everyone a larger download of
  Silo, including people who never make a Linux computer.

Recommended: Ubuntu, downloaded the first time. Many people start with main alone, and the wait
comes once, when a Linux computer is first needed.

## Notes

| | Linux computer | macOS computer |
|---|---|---|
| Image | One Ubuntu image, downloaded once. | Downloaded and installed on each Mac. |

The answer above is these two cells, written as one text.

Apple forbids shipping a pre-installed macOS image. Each Mac downloads and installs its own, so
the first run takes longer than with a Linux image (`first-run-time-macos`).

## A macOS computer that starts with nobody clicking (24 November 2026)

An agent's part, 24 November 2026. Read, not tried: this needs a Mac on macOS 27. After Apple's
installer runs, macOS starts on Setup Assistant, which asks for a user and leaves SSH off; tools
such as Tart get past it by typing keystrokes over the screen, which breaks when Apple changes a
screen. macOS 27 adds a way through it (WWDC 2026,
[session 224](https://developer.apple.com/videos/play/wwdc2026/224/)):
`VZMacGuestProvisioningOptions`, with `fullName`, `username`, `password`, `logsInAutomatically` and
`enablesRemoteLogin`, set on `VZMacOSVirtualMachineStartOptions` and passed to the first start.
Apple: "these parameters are automatically passed to Setup Assistant", and "these settings are
only honored if the guest has not already been set up". A fleet vendor's reading (Bitrise) says
both the Mac and the guest need macOS 27.

So main can be made with its user and SSH on from the first start, the way Silo reaches it
(`computer-reached-over-ssh`), with nothing typed into the screen. On a Mac before macOS 27, or a
guest before it, the keystroke way stays the only one known. What to try on a Mac: the time from
restore to an SSH login, and whether a password is needed at all once Silo puts its key in.

## With Xcode

An agent's note, 24 November 2026. For a Mac or iOS developer, the installed state worth keeping
includes Xcode and a simulator runtime, over 20 GB that every later macOS computer would otherwise
fetch again. `xcode-in-a-computer` proposes keeping it the way this answer keeps the installed
system. Mac test 23 of `research/2026-11-24-mac-and-ios-developers/README.md` measures it.

## Ubuntu's image as it is

An agent's part, 25 November 2026, from [option B's specs](../../../research/2026-11-25-linux-main/README.md). Read on that day: Ubuntu 26.04's cloud image for arm64
is a qcow2 file of 943 MB, checked against Ubuntu's `SHA256SUMS`, with 95 % of its clusters
compressed; root in partition 1, `/boot` in 13, EFI in 15. A computer boots a raw disk, so the copy
Silo keeps is the converted raw disk, and its hash can be pinned too: proposed words after "checks
it against Ubuntu's published checksum and keeps it": "converted to a raw disk whose checksum Silo
also pins,".
