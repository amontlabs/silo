# What happens to running computers when the engine stops

## For the owner

An agent's part, 22 November 2026. An agent is three hours into a job in main when Silo's engine
crashes.

The options:

- Computers keep running (recommended). The job goes on, under the rules main had, and a request
  that was held still ends at its limit. Until the engine is back you can change nothing: no fork,
  no restore, no new rule, and no way to pause or stop main through Silo. It also costs one extra
  small program per computer, which is what keeps the computer alive without the engine.
- Computers stop with the engine. Nothing ever runs while the engine is away. It costs every running
  program in every computer at each crash of the engine: the agent's job ends midway.

Recommended: computers keep running. Losing an agent's work to a fault in Silo is the worse of the
two, and the rules still apply while you wait. Whether the engine finds its computers again after it
restarts is still to find out by a trial.

- Suggested in the notes.
- If the engine stops, in the record's table of processes: "Changes wait. A hold still ends at its
  limit, which the helper keeps." A hold's limit is `hold-limit`, in the part `boundary`.
- Whether a restarted engine can take its helpers back is `engine-takes-back-helpers`.
