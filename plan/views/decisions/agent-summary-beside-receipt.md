# The agent's summary

## The choice

An agent's part, 22 November 2026. codex ends a run in a fork with "Fixed the login bug, all 212
tests pass." The receipt of the same run shows that `npm test` ended with 3 failures.

The options:

- Beside the receipt, marked as the agent's (recommended). You read the agent's account next to
  Silo's, and see at once where they disagree. It costs room on the screen, and a text you have to
  read knowing it can be false.
- Left out. The receipt holds only what Silo saw or was told. It costs the agent's reasons, which
  the record cannot give: why it chose one fix over another, or what it left for later.

Recommended: beside the receipt, marked. The agent's reasons are often the fastest way to the why,
and next to the record a false claim stands out.

## Notes

Marked Notes in the design record: a proposal from the plan's notes that the design relies on.
"It" is the receipt.

An agent's part, 24 November 2026, from the prototype in `research/2026-11-24-morning-review`. Both false claims of the night
were caught by a classifier with two kinds of claim: "tests pass", checked against the exit of
the last run of the whole suite, and "only changed files in the repo", checked against file facts
outside it. «All tests pass.» stood beside an exit of 1; «I only changed files in the repo.»
beside 401 files outside it. A claim it cannot check shows as not checked, never as true.
