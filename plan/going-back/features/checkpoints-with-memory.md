# Checkpoints with memory

## The choice

An agent's part, 22 November 2026. Main is a macOS computer where an agent drives an iPhone app
through a long test in Xcode, with a dev server open. You restore main to the 15:38 checkpoint.

The options:

- Build it, later (recommended). Some checkpoints also keep what was running, and after the restore
  Xcode, the app under test and the server are where they were at 15:38, as when a Mac wakes from
  sleep. It costs disk: each such checkpoint stores the computer's memory, several GB, so only a few
  can keep it. On a Mac, Apple refuses a saved memory state after some updates of macOS, and a fork
  can never use one. On Linux it waits for the virtual machine library Silo runs on.
- Leave it out. Every restore and every fork starts the computer again. It costs what lived only in
  memory: where the test had got to, the open apps, and any program's unsaved work. The agents'
  conversations still come back, since they are on the disk.

Recommended: build it, later. A cold start loses what an agent had on the screen, and on a macOS
computer that is often where the work is.

## Notes

From the plan's notes: a checkpoint that also keeps running programs (memory) comes later. On
Linux a checkpoint is a snapshot of the disk, with memory later. On macOS a memory state can be
saved, and is refused after some updates of the host.

## What a memory state brings back that should not come back (24 November 2026)

An agent's part, 24 November 2026. Read from Firecracker's documentation on snapshots
(`docs/snapshotting/random-for-clones.md` and `snapshot-support.md`) and from Node's source
(`lib/internal/crypto/random.js`, v22). Not measured: there is no virtual machine here to try it on.

Two things in a computer's memory are wrong the moment it is brought back later, or brought back
twice. Both apply to this feature and to forks of a running Linux computer (`forks-of-running-linux`).

**The clock.** A restored computer resumes with the time of the checkpoint. Firecracker says it in
as many words: the guest's wall clock continues "from the moment of the snapshot creation", and the
guest has to set it right itself. Restore main to 15:38 on Tuesday from Friday, and until the clock
is corrected every program sees Tuesday: a login token that expired on Wednesday looks valid until
the provider refuses it, TLS checks run against the wrong date, and a build compares file times that
are three days off. Correcting it is a jump, which Firecracker warns "may cause issues within the
guest". Proposed: Silo sets the guest's clock to the present as the first thing after a restore,
before the agents run, and the record notes the jump.

**Randomness.** A memory state holds the state of every random number generator in it. Restore it
twice, or fork it, and both copies draw the same numbers. Linux 5.18 and later reseed the kernel's
generator when the hypervisor signals a new generation (VMGenID), but Firecracker notes that
programs which keep their own pool "suffer from the same potential issues". A concrete case: Node
draws `crypto.randomUUID()` from a cache of 128 UUIDs filled in one go. Claude Code and Codex make
ids for requests, tool calls and conversations; in a fork of a running computer, main and the fork
hand out the same next ids, up to 128 of them, and `recognising-a-conversation` already found that
both sides of a fork send the same session ids. Math.random in every JavaScript program keeps the
same sequence for as long as it runs.

What follows for Silo:

- The virtual machine library must offer VMGenID, or Silo must reseed the kernel's generator in
  the guest before anything runs (Firecracker gives the code). Whether libkrun's snapshot work and
  Apple's Virtualization framework signal a new generation is to find out; Apple's pages on saving
  and restoring a machine state could not be read here.
- No reseeding reaches a running program's own pool. For forks of a running computer, either Silo
  accepts that the two copies may repeat ids and keys made from such a pool, and says so, or the
  agent programs are restarted in the fork. The second costs the running reply, which is most of
  the point of keeping memory. A restore, where only one copy runs, repeats numbers already used
  once to the outside: a one-time value sent before the checkpoint can be sent again.
