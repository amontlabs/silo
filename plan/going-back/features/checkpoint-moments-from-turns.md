# Checkpoints at each turn

## The choice

An agent's part, 22 November 2026. At 15:12 an agent runs a migration that drops a table. The last
interval checkpoint is from 15:05, and another agent has done seven minutes of work since. With a
checkpoint at each turn, there is one from 15:12, just before the migration.

The options:

- Build it (recommended). Each turn of an agent is a moment to go back to, and a restore around a
  command uses the moment before it. It costs disk for many more checkpoints, which thinning keeps
  down, and a turn waits when a checkpoint is slower than the model's reply. It works only for agent
  programs whose model traffic Silo can read.
- Leave it out. Checkpoints stay at intervals and asks. It costs precision: going back lands minutes
  earlier, and maybe in the middle of a command.

Recommended: build it. The moment before an agent's command is the one people want back, and the
interval checkpoints stay for the programs Silo cannot read.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

It is small: two decisions. It was split off so that the first checkpoints do not wait for model traffic to be read.
