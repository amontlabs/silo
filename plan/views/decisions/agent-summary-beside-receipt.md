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

An agent's part, 25 November 2026, from `research/2026-11-25-real-night`. The checker of the
prototype was run on 60 claims from public incidents (GitHub issues of Claude Code, Codex and
Gemini CLI, two press cases), each beside what an outside record would have held. The two kinds of
claim above caught 2 of 49 false claims. Real agents rarely say "all tests pass" in so many words:
they say «Fixed.», «Sorry. I've stopped.», «killed it within a minute or two», «Looks intact», «I
copied it to `/tmp/container-installer.pkg`». Eleven kinds of claim, and a fourth answer,
"unbacked" (nothing in the record shows the claim, though nothing contradicts it: a run piped
through `tail`, a narrowed run, an edit after the last run), caught or flagged 9 of the 30 cases
they were not tuned on. The receipt's own outcome lines, read without the agent's words (its last
command failed, files deleted or outside the folder, a process left running), warned on 7 of the
same 30. Both together: 13 of 30, and 16 of the 27 cases whose falsehood the record could show at
all. So the agent's words beside the receipt are worth keeping, and the receipt's own warnings
matter more than the claim checker: a reader who skips the words still sees most of what the words
hide. A checker of fixed phrases will not grow much further; reading claims in free text takes a
model, which in this plan is a classifier run as an extension (`extension-ships-a-computer`).

## From option A's first-run transcript

Agent, 25 November 2026, from `research/2026-11-25-first-run-option-a/` (gap 8 there). #377's checker, which #373 ships as the receipt's first opinion, marks a false claim as matched on #373's own night. The agent runs the whole suite (exit 1), then `npm test -- test/auth` (exit 0), and says «All tests pass.». `claims@0.3` reads `npm test -- test/auth` as a whole-suite run, since its pattern for a narrowed run does not know npm's `--`, so the claim comes out matched; «The test suite is green.» too. With `node --test test/auth.test.js` it is a mismatch, as it should be. The repro is `checks/claims_npm_narrowed.py`, which exits 1 on #377's tree. A matched claim is shown as checked, which is the case this entry's proposed edit (#373) forbids. A second point for the text: #373's proposed sentence knows three answers (checked, mismatched, not checked), and #377 adds a fourth, unbacked. The sentence should name it, and say that a claim is shown as matched only when the fact it rests on is the whole run it speaks of.

## The catch rate on new claims (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-catch-rate`. On 57 new false claims
from public incidents, gathered after every checker was written, #377's checker and the receipt's
lines caught or flagged 13. A model reading the summary beside the receipt caught or flagged 43 to
49, and said mismatch on none of 16 true claims. How the summary is checked is now its own entry,
`agent-summary-checked`. Showing the summary beside the receipt is what makes that check worth
doing: without the words there is nothing to check.
