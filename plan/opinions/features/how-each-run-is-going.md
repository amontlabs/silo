# How each run is going

## The choice

An agent's part, 22 November 2026. You come back to main after lunch. Claude Code is working on your
site. Codex has been stuck for 25 minutes: `git rebase` opened an editor and waits for input. A
second Claude Code has run the same failing test 9 times in a row.

The options:

- Build it (recommended). Each run shows its state (working, waiting on the model, waiting on a
  command, idle or stuck), its stretches (orienting, thrashing or progress) and its repeats, as
  Silo's opinion. It costs some wrong opinions: a script that sleeps for ten minutes on purpose
  shows as stuck. The app you drive your agents from also shows whether its own threads are working,
  so part of it is shown twice.
- Leave it out. It costs the overview: you open each agent's app to see how it is going, nothing
  tells you a run is stuck or going in circles, and no precaution can act on it.

Recommended: build it. Silo sees every run from outside, whichever program or app started it, so it
is the one place that can tell you a run is stuck or repeating itself.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

It holds three classifiers that ship with Silo: the state of a run, its stretches and its repeats. They could be three features.

A run is stuck when it makes no model call and shows no process activity, while one of its processes
sleeps or waits for input. The stretches of a run are read from exit codes and from what was read
and written. That line comes from the plan's notes before 18 November 2026.
