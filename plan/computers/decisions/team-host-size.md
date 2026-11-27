# The host for a team of five

## The choice

An agent's part, 25 November 2026. A team of five rents one server for its computers. Each person
runs main with three agents, and a fork or two in the day.

What it rests on:

- Memory. The common load of `sizing-target` is a 16 GB Mac per person, so 80 GB for five. Measured
  here ([the research](../../../research/2026-11-25-shared-linux-host/README.md)): an idle Debian 12 computer held about 380 MiB; one Claude Code process
  held 295 MiB during a long session. Builds, tests and language servers inside main are most of
  the rest, and were not measured. With KSM off (`memory-merging-across-people`), nothing is
  shared between people's computers.
- Disk. #365 found 30 days of daily checkpoints take 250 to 410 GiB per main writing 2 GiB an hour.
  Five mains take 1.25 to 2 TiB, plus their disks.
- Processor. Agents mostly wait on models; builds come in bursts. Not measured.
- KVM. A server rented whole, or a cloud machine with nested virtualization (#366).

The options:

- 128 GB, 16 cores, 4 TB NVMe (recommended). The common load for five with room for forks and the
  host. It costs a server of that size.
- 64 GB and 2 TB. It fits five idle mains and fewer checkpoints. It costs forks in the day and
  about two weeks of checkpoints.
- Sized per team, measured on the host by Silo. It costs a team the first night to find out.

Recommended: 128 GB, 16 cores and 4 TB, and the measured maximum lowers what Silo shows, as on a Mac
(`running-computers-maximum`).
