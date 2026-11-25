# Is a turn a moment for a checkpoint

## The choice

An agent's part, 22 November 2026. At 15:12 an agent runs a migration that drops a table in main's
database. The last checkpoint taken at an interval is from 15:05. With a checkpoint at each turn,
there is one from 15:12, just before that command ran.

The options:

- At each turn (recommended). You can go back to the moment before any command an agent ran, and the
  record can say between which two turns a file changed. It costs disk for many more checkpoints,
  which thinning keeps down, and a turn waits when a checkpoint is slower than the model's reply. It
  works only for agent programs whose model traffic Silo can read.
- At intervals and asks only. There are fewer checkpoints. It costs precision: going back lands up
  to one interval earlier, and an interval can fall in the middle of a command that the agent then
  runs again.

Recommended: at each turn. A turn is when an agent decides to act, so the moment before it is the
one people want back, and agent programs that keep a way back, such as Cline, keep it at each step
too.

## How long a turn waits for its checkpoint (24 November 2026)

An agent's part, 24 November 2026, from the prototype in `research/2026-11-24-checkpoint-engine/`
(Linux, XFS, an 8 GiB disk image in 485,000 pieces). It changes nothing in the answer and gives the
number the answer leaves to find out.

- With a checkpoint made by cloning the image, the turn waits for the whole clone: 9.2 to 11.8 s
  there, and the computer's writes wait as long.
- When Silo sees the computer's writes (a bitmap of the blocks written, kept in libkrun's virtio-blk
  for a Linux computer, or in a disk Silo serves over NBD for a macOS one), the checkpoint's moment
  is a swap of two bitmaps between two writes: under 1 µs there. The reply can go on right after the
  swap; the blocks are copied into the checkpoint afterwards, while the agent works.
- A checkpoint then takes 12 to 16 s to finish copying on that disk, and the next one cannot take
  its moment before the last one finished. With three turns a minute, a turn that comes sooner gets
  no checkpoint of its own, and the record says so, as the answer already allows.
