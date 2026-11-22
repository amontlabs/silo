# Is a turn a moment for a checkpoint

## For the owner

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
