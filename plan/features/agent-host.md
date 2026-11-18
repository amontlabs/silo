## Proposed

- A named setup covers a layout of several machines: a work device, a device that hosts the agents and a server for overflow.
- Docs: The sizing advice says that for Rust work a fast disk matters before more memory, and that 16 GB of memory ran out on Rust builds.
- Docs: The sizing advice says several smaller NVMe disks beat one large one.

## From the old code

The old code had it only as an idea and how much can be reused is not known.

Lessons:
- Measured fork costs (about 8 s, 0.2 to 0.5 GiB) come from one M4 Max with 64 GiB; capacity on other machines is unmeasured.

Where it lived, at the tag legacy-2026-11-16:
- `docs/ROADMAP.md#L396`
- `docs/SiloUI-CONNECTIONS.md`
- `docs/forks-ci/PLAN.md`

Calls: `silo host init`, `silo host status`, `silo devices add`.
