# How a fork is kept from acting as its parent

## The choice

An agent's part, 22 November 2026. Main runs a job every night at 02:00 that emails your team a
report, and a Tailscale node that puts main on your private network. At 18:00 you fork main to let
an agent try something risky.

The options:

- A new identity, scheduled jobs off (recommended). The fork gets its own name and address. The
  nightly job does not run in it and its Tailscale node is off, so your team gets one email and main
  stays the one on your network. Services the agent needs, such as its database, run. It costs a
  step when you want a job to run in the fork, and a service Silo does not know still runs as main's
  twin.
- Every service off too. Only what the agent starts runs. It costs the try itself: the database and
  Docker are down in the fork until the agent starts them, so the fork does not behave as main does.
- An exact copy, as copying a virtual machine by hand gives. It costs the nightly email twice, and
  two machines that claim to be main on your network.

Recommended: a new identity, scheduled jobs off. A fork is for trying something, not for doing
main's work twice, and the services an agent tests against keep running. The promise becomes "A fork
starts with an identity of its own, and runs none of its parent's scheduled jobs until you turn them
on", since "never acts as its parent" cannot be kept for a service Silo does not know.

## Notes

Git versions files; Silo versions something alive. A file does nothing when copied. A computer
has an identity, scheduled jobs, open sessions and logins. That is why a fork must not act as its
parent.

## Making the changes before anything in the fork starts (24 November 2026)

An agent's part, 24 November 2026. The answer leaves open how Silo changes the fork before anything
in it starts. Tried on Linux, on a raw ext4 disk image holding the files a fork must change: the
image was edited from outside with `debugfs`, the e2fsprogs tool that writes into an ext4 image
without mounting it. The script is in the project files, `research/2026-11-24-fork-identity`.

- It works and is quick: a new machine identifier, name, `/etc/hosts` and SSH host keys, the
  systemd timers and crontabs moved aside to a folder Silo keeps for turning them back on, and the
  files of Tailscale, a CI runner and the agent programs' sign-ins removed. 0.7 s on an image of 2 GB
  and 0.8 s on one of 64 GB, most of it making the new SSH keys; `e2fsck` found the image clean.
- **A removed file is still on the disk.** A search of the fork's image found the Tailscale node key
  after its file was removed: removing frees the blocks and leaves the bytes. Any program with root
  in the fork reads the raw disk and gets main's key back, so the fork can act as main on your
  network after all. Zeroing the file's blocks in the fork's copy before removing it fixed it (1.8 s
  in all); with a disk that shares blocks with main, this writes only those few blocks of the fork.
  This holds for anything Silo leaves out of a copy by removing it, sign-ins included
  (`fork-and-sign-in`).
- The edit from outside needs a raw image and a tool that writes ext4. A qcow2 image needs it
  opened first, and a Mac has no ext4 of its own: e2fsprogs builds there, but this was not tried
  on a Mac. The other way is a step of Silo's inside the computer, run first at boot. It needs
  nothing of the host, but runs only if the computer's own start-up runs it first, which a disk
  the agent can write does not promise.

What I would add to the answer: a file Silo leaves out of a copy is overwritten in the copy, not
only removed.
