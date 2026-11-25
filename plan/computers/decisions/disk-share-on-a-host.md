# Sharing a host's disk

## The choice

An agent's part, 25 November 2026. Ana's main keeps 30 days of checkpoints and a runaway agent in
it writes a 200 GB log. The host's disk fills. #365 found that a full volume breaks a computer's
own writes, so every computer on the host would fail, Ben's included.

Measured on a 16 GiB Btrfs file system ([the research](../../../research/2026-11-25-shared-linux-host/README.md), `quota.sh`). Ana's main disk of
3 GiB, four checkpoints, 100 MiB written after each, then Ana writes until refused, then Ben writes
1 GiB:

| Limit | Ana's space as counted | Real space used | Ben's write |
|---|---|---|---|
| None | 15 GiB counted file by file, for 3.4 GiB real | 15.5 GiB, the disk full | refused, disk full |
| A Btrfs quota group of 6 GiB per person, holding their snapshots | 3.4 GiB, shared blocks counted once | 6.0 GiB | written |

Counting file by file (`du`, or a quota that counts each file's blocks) counted each checkpoint at
its full size, four times too much here. XFS's quotas were not available in this kernel, so how
they count shared blocks was not measured.

The options:

- A share per person, counted by the engine (recommended). The engine already knows which blocks
  each checkpoint changed (#365's map of changed blocks), so it counts each person's real space.
  At the share, it thins that person's oldest checkpoints, then refuses new ones. It works on XFS,
  which #366 prefers. It costs a count the engine keeps, and is not built.
- Btrfs quota groups. Measured to work. It costs Btrfs's slower writes for every computer (#366),
  and quota groups' own costs with many snapshots, not measured here.
- No share. It costs the case above.

Recommended: a share counted by the engine, with a reserve kept free for the host as #365 proposes.
A person at their share still has their own main refused, so the engine stops checkpoints before
that.
