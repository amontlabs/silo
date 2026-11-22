# How the result of a check is stated

## For the owner

An agent's part, 22 November 2026. Under the promise "A restore changes only what is inside the
computer", the site says: "Held in 1 million made-up histories of up to 200 steps, and in tests that
attack a real computer on macOS 26. Not checked on Linux yet. Silo 0.4, 3 December 2026."

The options:

- What was checked, and what was not (recommended). Each promise says how it was checked and what
  the check leaves out. It costs promises that show their limits, which read weaker than another
  product's "secure".
- What was checked only. It costs a reader who assumes the check covers more than it does, such as
  Linux when only macOS was tested.

Recommended: what was checked, and what was not. A developer trusts a claim whose limits are written
next to it.

## Notes

The record's list of properties says it again: results are stated as the runs covered, never as
"proven".

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Say what a check does not look at.** State which outputs a check compares, and add other checks
  for the rest. A run that only shows nothing crashed says so. Guillotine's step-by-step comparison
  skips memory bytes and nested calls, while its docs say it compares state.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/BUG_HUNT_REPORT.md?plain=1#L562))
