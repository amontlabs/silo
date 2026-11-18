## Proposed

- Rule: Each auto-merge rule states the worst a hostile agent could bring into main under it.
- A rule can mark a directory as needing a person's read, and changes there never merge on their own.
- Rule: Any automatic reviewer or merge gate has a stated scope and a limit after which it must answer, so it cannot hold a fork forever.
- Test: Give a merge gate a reviewer answer that starts with prose or holds nested objects; the gate rejects it without trying to interpret it.
- A ready preset of rules merges user-level settings on its own and leaves every system-level change waiting for the user.
- Merges that no rule let through are listed with those most in need of a person first.
- A merge made by a rule is undone when the checks fail after it, and its fork goes back to waiting for you.

## Open

- May a rule merge a fork on a model reviewer's verdict, a probability set against the rule's threshold, or only on facts the engine checks?
  - Suggested: Only on facts the engine checks, because a reviewer model reads the hostile agent's work and can be steered by it.

## Notes

- Example rule from the old app: `when: { only: [code], tests: pass }`.
- Always checkpoint main first so undo works.
- Rules could differ per agent.
- Test the matcher against recorded diff fixtures.
- Built and merged on the old app but carried by no feature.
