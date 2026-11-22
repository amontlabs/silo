# Which moment does a restore use around a tool call

## For the owner

An agent's part, 22 November 2026. At 15:12 an agent runs a database migration that should change 40
tables, and it fails at the 25th. You want main as it was just before. An interval checkpoint was
taken at 15:13, in the middle of the migration.

The options:

- The moment before the command (recommended). Silo restores the checkpoint taken just before the
  migration started. The one from 15:13 stays on the Timeline, marked, with a warning that the agent
  may run that command again. It costs the work a command had done when you go back: the first half
  of a long build runs again.
- Any checkpoint, unmarked. You pick from the Timeline as it is. It costs a database left half
  migrated with nothing to tell you so, and an agent that sees an unfinished command and runs it
  again, sending a second time whatever it had already sent outside.

Recommended: the moment before the command. A command cut in half leaves a state that nobody chose,
and nothing on the disk says what it had already done outside the computer.
