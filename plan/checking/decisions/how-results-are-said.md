# How the result of a check is stated

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
