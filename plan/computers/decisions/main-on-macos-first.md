# Which system main runs first

## The choice

An agent's part, 22 November 2026. You install Silo on your Mac. Main is where your agents will
live, and it is either a macOS computer or a Linux one.

The options:

- macOS first (recommended). Your agents get a Mac of their own: they can build and test Mac and iOS
  apps, use a Mac screen, and what you bring in from your Mac keeps its paths and tools. A Linux
  computer beside main is never a copy of it: it is a fresh computer. It costs a first run that
  downloads macOS from Apple, over 10 GB, and installs it. A macOS computer takes more disk and
  memory than a Linux one. Apple allows two macOS computers running on one Mac, so at most one fork
  of main runs beside it. A fork of main starts from a copy of main's disk, so the programs that
  were running in main are not running in it.
- Linux first. Main is lighter, gives idle memory back, and several forks can run beside it. It
  costs what makes Silo different: a Linux machine for agents is what cloud services already offer,
  and agents that build Mac software, Silo's own app included, cannot do it there.

Recommended: macOS first. A Mac of its own is what people already give their agents. On the Mac
you already have, it costs nothing by the hour.

## polarzero's words, 28 November 2026

Owner: "On os yes we can just do a linux first if that's faster."

An agent's part. The answer above now says so: a Linux main on the Mac in the first build, and the
macOS main after it. It is faster for the reasons of the section below. The options of "The choice"
were written on 22 November and still say what each system costs. polarzero has not taken this
entry, so it is still his to rule. In the order, `computers-linux-main-on-a-mac` took the place of
`computers-main-on-a-mac`, which now comes after the first build. The entry keeps its id.

## The first build and this choice

An agent's part, 27 November 2026, written when the answer was a macOS main first, polarzero's
lean of 18 November.

`first-build-shows-the-outside` proposes a first build that shows the boundary, the model traffic
and extensions before checkpoints. For that build a Linux main on the Mac arrives sooner, for
reasons that did not exist on 22 November:

- An extension that executes ships a computer (`extension-ships-a-computer`), and
  `small-computer-for-an-extension` proposes that it be a small Linux one. If so, the Linux driver
  is built in the first build whichever system main runs.
- The macOS main waits on macOS 27 on polarzero's Mac and on the Mac tests, none of which has run.
- How Silo learns which process did what in a macOS computer is still to find out
  (`processes-from-inside-macos`).
- Nothing has booted on either system. The prototypes ran in Linux containers.

What it costs is what the first choice says: a Linux machine for agents is what cloud services
already offer, and no agent builds Mac software in it.

One way to keep both: the first build runs a Linux main to show the boundary, and the macOS main
follows as the product's main once its Mac tests pass. That would need `main-on-linux` and
`computers-linux-main-on-a-mac`, whose suggestion today is to drop it. It is polarzero's ruling.

## Notes

polarzero said this, 18 November 2026. polarzero expects much more interest this way.

## Why, as put forward

1. The visible thing people already do is give their agent a Mac of its own.
2. A macOS computer is what Linux sandboxes in the cloud cannot offer.
3. Agents that build Mac or iOS software need macOS. That includes the agents that build Silo's
   own app.
4. Under this design a fork is occasional. The limit of two macOS computers per Mac does not rule
   it out.

Reason 2 no longer holds as written. Agents' research of 24 November 2026 found cloud services
that rent macOS machines to agents by the hour, and local apps that give an agent a macOS virtual
machine. The recommendation rests on reasons 1, 3 and 4, and on the cost: a macOS computer in main
runs on a Mac the user already owns.

## Facts it has to respect

From earlier work on macOS computers. Not yet a decision.

- Apple allows two macOS computers running on one Mac. With main on macOS, one macOS fork can run
  beside it.
- A fork of a macOS computer is a clone of its disk and boots cold. A saved memory state cannot
  move to a new identity under Apple's framework.
- Apple forbids shipping a pre-installed macOS image. Each Mac downloads and installs its own, so
  the first run takes longer than with a Linux image (`first-run-time-macos`).
- A macOS computer needs a second virtual machine monitor, Apple's Virtualization framework, driven
  from Swift on the engine side. That is decided, in `engine`. Its network goes through Silo's
  boundary, never Apple's NAT, as `engine` puts forward.
- A macOS computer is larger than a Linux one, in disk and in memory.

## What a fork is beside a macOS main

polarzero said "forks are macOS or Linux". A fork is a copy, so a Linux computer beside a macOS
main is a fresh computer, not a copy, and the answer says so. `linux-beside-a-macos-main` says
how the code reaches it.

## What follows from it, put forward

`linux-beside-a-macos-main`, `containers-on-macos`, `record-halves-per-system`,
`macos-ships-facts-first`, `x86-linux-computers`, `neutral-core`.

## Who needs it most, 24 November 2026

An agent's part, from research on who would use Silo first. It adds a reason and changes neither
the answer nor the recommendation.

- A Linux computer per agent, with network rules and secrets kept outside, is already free on a
  Mac. Agents that build Mac and iOS software cannot use one: Linux has no Xcode and no iOS
  simulator. For these developers the alternatives are a second Mac or a Mac rented by the hour or
  the month. Main on macOS is where Silo has no free substitute.
- A developer shipped a small app on 20 November 2026 to watch agents inside macOS virtual
  machines, because "I couldn't see what the agents were doing in there". That is the record, asked
  for by someone who already runs agents in a macOS computer.
- Several agents in one macOS computer clash over the iOS simulator. `simulators-for-parallel-agents`
  asks what Silo does about it.

## The limit, from Apple

An agent's note, 24 November 2026. Apple's developer support said in September 2026 that the
framework enforces the limit of two for macOS guests only, and "has no fixed cap for Linux VMs".
It declined to say how to get the written exception the license mentions. For an iOS developer, the
parallel work then happens inside main, which is what `simulators-for-parallel-agents` is about.
Details: `research/2026-11-24-mac-and-ios-developers/README.md`.

## Weighed against a Linux main first

An agent's part, 24 November 2026, from [the comparison of first builds](../../../research/2026-11-24-first-build-and-the-record/README.md). A Linux main on the Mac in the first build was
weighed, because more of the record has already run on Linux (#347, #367, #371) and a Linux
computer can give processes from a probe with a kernel of Silo's own. It is not recommended: it
lands beside free Linux microVM products on the Mac, it gives up Xcode and macOS apps in main, and
the macOS main would still have to be built later, which writes the computer, its checkpoints and
its file comparison twice. It stays the fallback if the Mac kit (#359) shows that macOS 27's
provisioning or gvproxy under a macOS computer does not work: the engine, the boundary and the
recorder run on the Mac outside every computer and carry over unchanged.

## Option B specified

An agent's part, 25 November 2026, from [option B's specs](../../../research/2026-11-25-linux-main/README.md). B, a Linux main on the Mac, is now specified at the
level of #370 for a macOS main, in steps 2 to 7, with a test kit for a Linux host with KVM. Under
B this entry's answer would read "Main is a Linux computer first. Its forks are Linux. A macOS
computer comes later, as a computer of its own beside main", and `linux-never-copies-macos-main`
stays as it is. Three things the notes above do not weigh: B's Linux computer is the one
`computers-linux-beside-main` needs anyway, so building it first loses nothing under this answer;
a macOS main has more ways to fail before the first night (macOS 27, provisioning, gvproxy with a
macOS computer, the Keychain over SSH); and B's first morning holds processes and Docker. The
recommendation stays this answer, with an order of building that leaves the choice open until the
Mac's tests have run.
