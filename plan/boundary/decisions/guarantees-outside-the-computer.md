# Where is what Silo guarantees done

## For the owner

An agent's part, 22 November 2026. A rule says main never reaches paste.rs. An agent in main uses
sudo to stop the part of Silo that runs inside main, changes Claude Code's own settings, and tries
paste.rs.

The options:

- Only what Silo keeps from outside (recommended). The connection is still denied and the attempt is
  still in the record, because both are done outside main. What comes from inside main, such as
  which process made the attempt, is shown as unverified and promised nothing. Each promise an agent
  with root could break is tested with a hostile program before Silo makes it. It costs the promises
  Silo could make if agents behaved: which process or which agent did something is never one of
  them.
- Also what holds while the agent does not fight it. Silo also promises what its parts inside the
  computer keep, and what the agent program's own settings keep. It costs the promise itself, since
  an agent with sudo breaks it with one command, at the moment it matters.

Recommended: only what Silo keeps from outside. The agent is treated as hostile and has sudo, so it
can break anything Silo keeps inside the computer.
