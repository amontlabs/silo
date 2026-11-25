# What a receipt holds

## The choice

An agent's part, 25 November 2026, from the challenge of #355 (page 7) and the prototype of #371.

The case: an agent runs `npm test | tail -20`, three tests fail, and the agent writes "All tests
pass." The shell's exit code of that pipeline is 0, since it is `tail`'s. A receipt that shows "the
commands that ran, with their exit codes", as the earlier answer says, shows a 0 beside the false
claim. #371's first screen caught "All tests pass." when the suite had exited with 1, because it read
the exit code of the test runner itself.

The options:

- Show the test runner's own exit code and the pipeline's, whether the run was narrowed, whether a
  file changed after the pass, the changes that weaken checks, and the processes still running
  (recommended). #371 found that two of four runs left a process running that nothing in their
  words mentioned. It costs the probe reading the exit code of each process, which it does already
  on Linux; on macOS it waits on `processes-from-inside-macos`.
- The commands with their exit codes, as the earlier answer says.

Recommended: the runner's own exit code and the rest. Marking each claim of the agent as backed,
stale or contradicted is an opinion, proposed by #355 as a decision of its own
(`claims-against-the-record`), and is not part of this answer.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

An agent's part, 22 November 2026. A task ran for 1 h 12 in a fork: "fix the flaky login test". You
open its receipt.

The options:

- The full account, outcome first (recommended). Your words, the test results, the 38 files changed
  and the one only in the fork, then the commands, the hosts and the secret used, $6.40 spent, 50
  minutes of work and 22 of waiting, and a gap of 2 minutes. It costs a long page, read from the
  top; most of the time you stop after the first lines.
- A short account. What you asked, the files changed and the spend. It costs what you need when
  something looks wrong: which hosts it reached, which secrets it used and what Silo missed.

Recommended: the full account, outcome first. The first lines let you judge the work in a minute,
and the rest is there when something looks wrong.

## Notes

The design record gives this as a list with no mark. Its first item, "the request that started
the run", is written here "what you asked the agent": "request" names only what waits at the
boundary.

An agent's part, 24 November 2026, from the prototype in `research/2026-11-24-morning-review`. Two of four runs left a process running
that nothing in their words mentioned (`node scripts/sessiond.js`, `bun test --watch`). The
receipt's outcome showed it from the probe's reports: "1 process it started still running". The
outcome also shows the last run of the whole suite apart from the last test command of any kind,
since a run that ends on a narrower test is the case that misleads.
