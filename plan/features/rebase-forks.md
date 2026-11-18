## Proposed

Why: An open fork drifts from main until its merge is the hard case.

- When main changes, the agent in each open fork is told, and a fork behind main is flagged before it asks to merge.
- Rule: Main's changes come into a fork through the same item-by-item merge, with conflicts asked.

## Open

- Does a fork take main's changes only when asked, or by itself whenever main moves?
  - Suggested: Only when asked, by you or by its agent, since a fork that changes under a running agent surprises it.
