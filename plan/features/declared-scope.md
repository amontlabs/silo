## Proposed, not confirmed by the owner

Why: Changes outside the task, such as an agent editing its own instructions, hide among ordinary changes until merge.

### For the owner to settle

- Is the list set per computer (files no fork changes, such as AGENTS.md), per fork (what the task may touch), or both?
  - Recommendation: Per computer first, since it is one short list and it guards the agent's own instructions.
- Does a change outside the scope fail the fork at once, or only block its merge?
  - Recommendation: Block the merge; the agent is root in its computer, so the merge is where Silo can refuse.

### For whoever builds it

- Rule: A change to a listed file blocks the merge, whatever auto-merge-rules allow.
- Detail: The diff flags every change outside the fork's declared scope.
- Detail: Declared scopes let the overlap between forks be known before work starts.
