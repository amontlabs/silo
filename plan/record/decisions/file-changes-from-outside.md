# File changes, seen from outside

## For the owner

An agent's part, 22 November 2026. An agent runs a build with 8 jobs in a computer, then unpacks
50,000 small files. Two trials on 21 November measured both ways of seeing those changes from
outside.

The options:

- Compare at each checkpoint (recommended). Nothing is slowed. The record learns of file changes at
  each checkpoint, not as they happen, and a file created and deleted between two checkpoints never
  appears. It costs the live view of files, and 3 to 22 seconds of comparing per checkpoint for
  200,000 files in the trials.
- Watch every file operation live. Every create and delete is seen as it happens, and none is lost.
  It costs a build 1.6 to 2.1 times slower, and the 50,000 files 12.6 to 192 times slower in the
  trials.

Recommended: compare at each checkpoint. A computer that builds twice as slowly will not be used.
Neither way has been measured on a macOS computer, which main is, so this is a lean until then.

## Notes

To find out, on Linux and on macOS computers. It is the main research question for the record.
What each route costs is part of it.

Two routes:

- **The engine serves the work folders itself.** Every file operation passes through it, live.
  The cost is speed, exactly where builds run. A folder served from outside the computer, or a
  filesystem layer, is measured before it is adopted, and a slowdown of eight times on parallel
  file work is the failure to avoid. (This last sentence is in the plan's notes; it is a
  proposal.)
- **The engine reads the disk at each checkpoint and compares.** Nothing is slowed while the
  computer runs. The result is only as fine as the checkpoints, and the engine has to read the
  computer's filesystem.

## Found

Two trials on 21 November 2026 measured both routes on Linux computers started with MicroSandbox.
Nothing that runs inside the computer was used as a source. The code is thrown away, as every
trial's code is.

- **The Linux machine:** Ryzen 7 2700X, Ubuntu 26.04.1, MicroSandbox 0.7.14. Each number is the
  median of 7 interleaved runs.
- **The owner's Mac:** M4 Max, macOS 26.5, MicroSandbox 0.7.16. Each number is the median of 5
  interleaved runs. All of them were taken while other trials ran on the same Mac, at a load
  average of 46 to 184. Its ratios are worth more than its times.

### The engine serves the work folders itself

| | Linux machine | Mac |
|---|---|---|
| 50,000 small files unpacked then deleted, with 8 jobs | 12.6 times slower than on the computer's own disk | 192 times slower (77 to 912 under load; 174 in an earlier pass at a lower load) |
| A build with 8 jobs | 1.6 times slower (BusyBox) | 2.1 times slower (Git) |
| What the host sees | every create as it happens, none lost, through fanotify | every create and delete, none lost, through FSEvents |

- The failure to avoid, eight times slower on parallel file work, is passed on both machines.
- An event names the path and the computer. It never names the program inside the computer.
- On Linux, watching costs about 2%. A write the computer has not saved to its disk yet reaches
  the host at once.
- MicroSandbox has no setting that makes the served folder faster, and no hook to see each
  operation in its own file server. Both were read in its code, not measured.
- On the Mac, a program built in the served folder could not be run: "Permission denied". This is
  MicroSandbox's issue 2121, a regression of 0.7.16, open on 21 November 2026 with a fix proposed
  in its pull request 2125. The issue was reported on a Linux host. Nobody has reported it for
  macOS.

### The engine reads the disk at each checkpoint and compares

| | Linux machine | Mac |
|---|---|---|
| Taking a checkpoint | the computer pauses 3.1 s while 955 MiB of disk is copied | 1.6 to 1.9 s |
| Comparing two checkpoints of a tree of about 200,000 files | 3.3 to 3.9 s | about 22 s, or 15 s when the earlier list is kept |
| How the host reads the disk | it mounts a copy read-only, or reads it with `debugfs` without mounting | a second computer mounts both checkpoints read-only |

- The time follows the size of the tree: 10, 1,000 or 50,000 changed files cost the same.
- Every changed file was found.
- A file created and deleted between two checkpoints is missed.
- A file written and put back shows only through its change time, and root inside the computer
  can set the clock.
- What the computer has not saved to its disk yet is missed. On Linux, 1,000 such files were
  invisible after 5 s and visible after 40 s. Root inside the computer can delay that saving.
  Asking the computer to save first runs a program inside it, so the trials did not ask.
- A rename cannot be told from a delete and a create by the file's inode number, because ext4
  gives a deleted file's number to the next new file. On Linux, 4,939 of 10,000 deletes were
  taken for renames.

### A third route: the blocks written between two checkpoints

- On the Mac, each checkpoint adds a layer that holds exactly the disk blocks written since the
  last one. The host reads them in about 0.1 s with no computer running. Turning blocks into
  files is not built.
- On Linux, comparing two copies of the disk block by block took about 4.4 s. Naming the changed
  files then took 0.25 s for 10 of them and 32.5 s for 50,000. It sees no delete, and no change
  to a file's attributes alone.

### Not checked

- macOS computers: none exists on the Mac. On paper, Apple's Virtualization framework offers both
  routes: a shared folder (`VZVirtioFileSystemDeviceConfiguration`) and a disk image the host can
  copy and read (`VZDiskImageStorageDeviceAttachment`).
- The Mac's numbers on an idle machine.
- A faster way to serve a folder.
- The risk of the host mounting a disk that a hostile agent wrote.
- A checkpoint taken while the disk is busy.
