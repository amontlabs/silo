# Stopping one run

## For the owner

An agent's part, 22 November 2026. Sixteen agents work in main. One run of Codex is stuck in a loop
of `cargo test` with twelve child processes. You stop that run from the app, and the other fifteen
agents keep working.

The options:

- Build it (recommended). Silo ends the agent program and everything it started. It costs certainty:
  which processes belong to a run is told to Silo from inside main, so a process that detached
  itself can survive, and on macOS that information may not exist at first. What the run held in
  memory is lost.
- Leave it out. You end it from inside main, with `kill` over SSH, or you stop all of main. It costs
  time, or the other fifteen agents' work.

Recommended: build it, in its later stage. One stuck agent should not cost the others.

## Notes

From the list of smaller features the owner put on the roadmap on 21 November 2026. The owner's
words on the list are in `what-ships-with-silo`.

## An agent's notes

- `computers-stopped-and-paused` ends everything in a computer. This ends one run and leaves the
  others.
- "Stop" is filed as destructive in `kinds-of-action`, for a whole computer. Whether ending one run
  is the same action, and whether "stop" is the word for it, is to settle
  (`kinds-of-other-actions`).
- If it is destructive, only you or a rule does it, and no tool of an extension does
  (`what-an-extension-never-does`). So this is part of Silo and not an extension.
- Which processes descend from a run is known from reports from inside the computer
  (`processes-in-the-record`). "Everything it started" is as sure as those reports.
- Bad paths to design: a process detached itself from the run's tree. Two runs use one process, such
  as a server one of them started. The agent program was writing its conversation when it ended.
