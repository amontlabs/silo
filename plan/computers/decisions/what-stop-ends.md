# Which processes stopping a computer ends

## The choice

An agent's part, 22 November 2026. You stop main. A database is running in it, and an agent left a
development server running in the background with `nohup`.

The options:

- Every process, after a short wait (recommended). The database and the agents get up to 30 seconds
  to save and close, and nothing crosses main's boundary meanwhile. Whatever still runs after that
  ends, the server started with `nohup` included. It costs up to 30 seconds for each stop.
- Every process, at once. Like pulling the power cord. It costs what was being written: a file can
  be left half-written, and the database may need repair at the next start.

Recommended: after a short wait. Thirty seconds is what Kubernetes gives a program to stop, and
since the boundary closes first, a stop that reacts to an attack still cuts the computer off at
once.

## Notes

polarzero called this useful. That is not a yes.

The property put forward is the shorter sentence, about stopping only. Dropping, and the bin, are
in `going-back`.
