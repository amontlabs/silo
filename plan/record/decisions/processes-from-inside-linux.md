# Processes from inside, on Linux

To find out, with how far a hostile agent can interfere with each means.

- Linux has the means: the process tree, accounting per group of processes, file events that name
  the process. Which to use, and what a hostile agent can do to each, has to be established.
- On Linux the kernel can come from outside the computer's disk. If it also refuses changes, its
  word is worth much more than a program's. It is still a report, and a rule still never reads it.
  (To find out)

## polarzero's question (20 November 2026)

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

## polarzero's direction (20 November 2026)

polarzero accepts that this research belongs to the Workshop stage, with one direction for it: as
many facts as possible, which cannot be tampered with, and as few reports as possible. A report is
used only for what can be known only from inside the computer.

## In a first build with a Linux main

An agent's part, 25 November 2026, from [option B's specs](../../../research/2026-11-25-linux-main/README.md). Its step 7 builds the probe as this answer says, on a
kernel of Silo's own configuration (eBPF on tracepoints, BTF, the process connector; no modules,
no kexec, no `/dev/mem`). Under polarzero's direction of 20 November, it proposes one sentence
more: "Before the probe is relied on, a report of each exec and exit made by Silo's own kernel
through a call to the helper is measured against the same target; it cannot be stopped by root
inside without a kernel exploit, and where it meets the target it replaces the probe for starts and
exits." The case: an agent with root stops the probe, then deletes a folder; the record has a
silent source and file facts with no process behind them.
