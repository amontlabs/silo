## Proposed, not confirmed by the owner

### For the owner to settle

- Can a template be a process of several steps, such as an implementer then a reviewer, run durably by the engine, or is it one task's instructions?
  - Recommendation: One task's instructions at first, since the agent program inside already runs its own subagents and Silo's part is the computers they use.

### For whoever builds it

- Rule: Reusable prompts, scripts and snippets come in two layers, main's for every project and one per repository, and Silo keeps both outside the repository.
- Detail: Something a fork found outside its task becomes a follow-up task in one action.
- Detail: A recurring task can be written in plain words, and the agent turns it into a script that then runs on schedule without an agent.
- Detail: Each template appears in the app as a named button that starts it.
- Detail: The user writes templates of their own in the same form as the shipped ones.
- Detail: A shipped template upgrades one dependency in a fork, and the fork's diff shows the result.
- Detail: A shipped template adds a dependency by auditing it in a fork, pinning its version and showing it in the diff's package group.
- Detail: A shipped template turns a review comment that keeps coming back into a check.
- Detail: A shipped template checks a project's bundle size.
- Detail: A scheduled template watches named reference repositories and proposes what to port from them, such as a feature, a refactor or a patch.

## From the old code

The old code had it only as an idea and how much can be reused is not known.

Where it lived, at the tag legacy-2026-11-16:
- `docs/product/JOURNEYS.md#L49`
