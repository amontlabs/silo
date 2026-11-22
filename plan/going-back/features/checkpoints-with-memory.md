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
