## Proposed

- A recipe's after-merge step or check can name the paths it depends on, and it runs only when the merge changed one of them.

## Open

- Do a recipe's steps also run at a fork, at a checkpoint and before a merge is offered, or only after a merge as planned?
  - Suggested: Only after a merge for now, since checks before a merge belong to honest-checks and quiet-forks already covers what a fork turns off.

## From the old code

The old code had it built, about 300 lines, and it needs reworking.

Lessons:
- Hooks a caller sends are ignored for a computer built from a recipe.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-RECIPES.md#what-a-computer-remembers`
- `docs/SiloUI-CHANGES.md`

Calls: `silo merge --data-ok`.
