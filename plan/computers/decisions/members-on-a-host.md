# Keeping people apart on a shared host

## The choice

An agent's part, 25 November 2026. Five developers share a Linux server. Ana runs main and two
forks; Ben runs main overnight with a long migration. Ana's forks start a large build and the
server runs out of memory.

Measured in a Linux container ([the research](../../../research/2026-11-25-shared-linux-host/README.md), `oom.sh` and `creds.sh`), with processes
standing in for computers. With every computer in one control group for the host, the kernel
stopped the largest process, which was Ben's main, though Ana's three computers caused the
shortage. With Ana's computers in a control group of her own, limited to her share, one of Ana's
computers was stopped and Ben's main went on. A process of another Unix user could not read the
memory of Ana's boundary process, nor use the socket of her SSH agent in a folder only she can
open.

The options:

- A Unix user and a control group for each person (recommended). It costs a user to create on the
  host for each person, and a memory share to set for each.
- One Unix user for the whole engine, with the engine keeping people apart. It costs every
  separation the kernel gives for free: any bug in the engine that lets one person's request name
  another's computer reaches it, and an out-of-memory stops whichever computer is largest.
- A virtual machine per person, with their computers nested inside. It costs nested
  virtualization, which most hosts lack (#366) and which slows every computer.

Recommended: a Unix user and a control group for each person. It is how a shared Linux machine
already keeps people apart, and it holds even when the engine is wrong.

What it does not cover, said plainly: the host's administrator (root) reads everything on the
host. See `secrets-on-a-shared-host` and `records-on-a-shared-host`.
