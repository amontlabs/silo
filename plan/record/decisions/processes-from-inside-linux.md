# Processes from inside, on Linux

To find out, with how far a hostile agent can interfere with each means.

- Linux has the means: the process tree, accounting per group of processes, file events that name
  the process. Which to use, and what a hostile agent can do to each, has to be established.
- On Linux the kernel can come from outside the computer's disk. If it also refuses changes, its
  word is worth much more than a program's. It is still a report, and a rule still never reads it.
  (To find out)

## The owner's question (20 November 2026)

Whether designing this can change the ground the films stand on. If it cannot, it belongs to the
Workshop stage.

## One means, read in the source (20 November 2026)

Where the engine serves a folder to the computer over virtio-fs, each file request already
arrives outside with the number of the process that made it, as the computer's kernel states it.
libkrun's `Context` carries `uid`, `gid` and `pid` (`src/devices/src/virtio/fs/filesystem.rs`),
and MicroSandbox serves host folders this way (`crates/filesystem/lib/backends/passthroughfs`).
The operation is seen from outside. The process number is the kernel's word.

Not checked: whether a write the kernel delays still carries its writer, and which means the
computer's kernel is built with (file events that name the process, groups of processes). Its
configuration is not on this machine.

## The owner's direction (20 November 2026)

The owner accepts that this research belongs to the Workshop stage, with one direction for it: as
many facts as possible, which cannot be tampered with, and as few reports as possible. A report is
used only for what can be known only from inside the computer.
