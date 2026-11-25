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

## From option A's first-run transcript

Agent, 25 November 2026, from `research/2026-11-25-first-run-option-a/` (gap 8 there). #377's checker, which #373 ships as the receipt's first opinion, marks a false claim as matched on #373's own night. The agent runs the whole suite (exit 1), then `npm test -- test/auth` (exit 0), and says «All tests pass.». `claims@0.3` reads `npm test -- test/auth` as a whole-suite run, since its pattern for a narrowed run does not know npm's `--`, so the claim comes out matched; «The test suite is green.» too. With `node --test test/auth.test.js` it is a mismatch, as it should be. The repro is `checks/claims_npm_narrowed.py`, which exits 1 on #377's tree. A matched claim is shown as checked, which is the case this entry's proposed edit (#373) forbids. A second point for the text: #373's proposed sentence knows three answers (checked, mismatched, not checked), and #377 adds a fourth, unbacked. The sentence should name it, and say that a claim is shown as matched only when the fact it rests on is the whole run it speaks of.
