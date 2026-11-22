# Which run did it

## The choice

An agent's part, 22 November 2026. Main holds two runs of Claude Code and one of Codex.
/work/site/src/app.ts was last written by the Codex run, which ended at 11:40.
/work/site/package-lock.json was written by all three today. A `git pull` you ran from your terminal
is yours.

The options:

- Build it (recommended). For each path the app says which run wrote it last and whether that run is
  still going, which conversation belongs to which run, which paths several runs wrote, and what you
  did yourself. It costs certainty: each answer is Silo's opinion, from which program started which,
  and an agent inside can bend that. What neither a run nor you explains
  stays unattributed.
- Leave it out. It costs the answer to "who did this?", and the features that need it: putting back
  one run's work, a budget on a run, and the map of hot files.

Recommended: build it. Which agent did what is the first thing people ask of a computer several
agents share.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

The opinion on hot files rests on `heat-is-built-in`, which has no verdict. Its notes say the owner agreed to the map of hot files, not to a classifier for it.

The run of a process and the run of a conversation are the joins of the chain (`the-chain`) that the
boundary does not see. A conversation itself is read from model traffic at the boundary. Which paths
many runs wrote is counted from the opinions on which run wrote each path, for the map of hot files
(`hot-files`). It is not a classifier of its own (`heat-is-built-in`).
