# How you go back on a main shared by several agents

## The choice

An agent's part, 25 November 2026, from the challenges of #354 and #348 to `fork-from-the-moment`, which
polarzero took, as #364 (section 9) reconciles them.

The case: at 03:00 Codex deletes `~/work/app/src`, which it never committed, while Claude Code works
through the night in another repository of main. The earlier answer says you do not rewind a shared
main: you fork from the moment, and the result leaves the fork through Git. But the folder was never
committed, so Git cannot carry it back, and the first build has no fork at all; the only way back is
a restore of the whole computer, which also undoes Claude Code's night.

Two decisions polarzero took read against each other here: `fork-from-the-moment` says you do not
rewind a shared main, and `one-run-grain` with `putting-back-a-runs-paths` puts a run's paths back
into it. #348 reads them as already allowing `silo put-back`; #354 reads the first as forbidding it.
Agents cannot settle which reading holds, since polarzero ruled both.

The options:

- Put back the paths you name, after a checkpoint of now, or fork from the moment (recommended).
  #348's `putting-back-a-path` and #354's `silo put-back` both rest on it. It costs one more way of
  going back to explain, and a put-back that overwrites what another agent wrote since, which the
  checkpoint of now lets you undo.
- Fork from the moment only, as the earlier answer says. It costs the case above until forks exist,
  and after that a fork and a copy out by hand for anything never committed.

Recommended: put back the paths, or fork. Without a decision either way, every later draft will
raise the same question again.
