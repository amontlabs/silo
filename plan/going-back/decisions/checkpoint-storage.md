# Where is a checkpoint stored

## The choice

An agent's part, 22 November 2026. An agent in main runs `sudo rm -rf /`, by mistake or on purpose.
It has full access inside main.

The options:

- Outside the computer, never changed (recommended). The agent can wreck main but cannot touch a
  single checkpoint, so you restore main to the moment before. It costs disk on your Mac:
  checkpoints sit on the same drive as the computer, and cannot move to an external drive without a
  full copy.
- Inside the computer, as snapshots of its own file system. Each computer keeps its own history, and
  an agent with full access can read, change or delete it. It costs the way back exactly when an
  agent goes wrong.

Recommended: outside the computer, never changed. The agent is treated as hostile, so the way back
has to be out of its reach.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **A checkpoint names its place in the record.** Represent a checkpoint as a position in an
  append-only log of changes. A checkpoint of a computer's disk also carries the record position at
  which it was taken, so "what happened since this checkpoint" is a plain query.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/state/journal.zig#L5))

## The folder as the catalog, and a full volume (24 November 2026)

An agent's part, 24 November 2026, from the prototype in `research/2026-11-24-checkpoint-engine/`
(Linux).

- **Whole or not at all, with no database.** Each checkpoint is a folder (`disk.img`, the macOS
  computer's `aux.bin`, `meta`), built under a name starting with a dot, flushed, then renamed into
  place; it is removed by a rename to another dotted name, then deleted. Starting the engine deletes
  every dotted name. Killed at each of its first 300 steps, this recovered every time, and the test
  caught two bugs planted in it.
- **A full volume breaks the computer's own writes, not only the next checkpoint.** Once the disk
  image shares blocks with a checkpoint, overwriting one of its blocks takes new space. On a full XFS
  volume, the computer's overwrites failed with "No space left on device"; deleting a 200 MB reserve
  file let them go on. Apple's `clonefile(2)` says the same of APFS. The answer could add a floor of
  free space: below it, Silo takes no checkpoint, thins the oldest and alerts, and the reserve file
  is what the computer writes into meanwhile. What Apple's framework does when the host refuses a
  write (an error inside the computer, or the computer stopped) is a test for the Mac.

## An external disk

An agent's part, 25 November 2026, from `research/2026-11-25-checkpoint-disk-use/`. "Cannot move to an external drive without a full
copy" holds for clones. When checkpoints are files Silo writes (#365's `delta`, or old blocks
copied before the write), the oldest move as files. In #389's simulation, 500 GiB on an external
disk gives a 512 GB Mac the full 30 days at 2 GiB an hour, and 16 to 17 days at 5 GiB an hour or
without TRIM. With zstd as well, 30 days in every case simulated.

What it needs: the files encrypted with a key in the Mac's Keychain, since anyone holding the disk
can read it; and the Timeline showing those checkpoints as out of reach while the disk is not
connected. A cloud store works the same way, at about 1.2 Mbit/s of upload for 2 GiB an hour
compressed.
