## Proposed, not confirmed by the owner

Why: An open fork drifts from main until its merge is the hard case.

### For the owner to settle

- Does a fork take main's changes only when asked, or by itself whenever main moves?
  - Recommendation: Only when asked, by you or by its agent, since a fork that changes under a running agent surprises it.

### For whoever builds it

- Detail: When main changes, the agent in each open fork is told, and a fork behind main is flagged before it asks to merge.
- Rule: Main's changes come into a fork through the same item-by-item merge, with conflicts asked.
