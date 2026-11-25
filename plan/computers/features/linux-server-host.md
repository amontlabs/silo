# A Linux server as host

## The choice

An agent's part, 22 November 2026. You run Silo on a Linux server with 128 GB of memory. Twenty
forks of a Linux main try twenty fixes at once, on Intel processors, and you follow them from your
Mac.

The options:

- Build it (recommended). Silo runs computers on a Linux server as on a Mac. It costs a second host
  system to build and test, and a way into Silo from your Mac to the server that has to be guarded.
- Leave it out. Silo runs only on your Mac. It costs the many-forks case and Intel Linux computers,
  and it goes back on what polarzero decided: that Silo also runs on Linux, and that computers run on a
  server after your own machine.

Recommended: build it, in its later stage. A Mac holds a few computers, and a server is where the
rest go.

## A host five people share (25 November 2026)

An agent's part, from measurements in a Linux container
([`research/2026-11-25-shared-linux-host`](../../../research/2026-11-25-shared-linux-host/README.md)).
It keeps the recommendation and splits what a host several people share needs into entries that
can be decided one by one:

- Keeping people apart: `members-on-a-host` (a Unix user and a memory limit per person),
  `memory-merging-across-people` (KSM off), `disk-share-on-a-host`.
- The network: `computer-known-by-its-link` (on a shared bridge, a computer that took a stopped
  computer's address got its owner's cloud token), `host-out-of-reach`.
- Secrets and records: `secrets-on-a-shared-host`, `records-on-a-shared-host`. The host's
  administrator can read both, and Silo says so.
- Spend: `pooled-budget-on-a-host`, exact on one host.
- Size: `team-host-size`, 128 GB, 16 cores and 4 TB for five.
