## Decided

- 2026-11-10, owner: a `.silo` folder in a repository on the Mac (a project-first flow) is not the product the owner wants.

## From the old code

The old code had it and then dropped it, about 1,500 lines, and it would be written again.

Lessons:
- Host git runs with hooks, fsmonitor and ext protocol off, and refuses .gitattributes changes, so fork commits cannot run host programs.
- Auto-keeping a fork when its tests pass was dropped: the tests run inside the fork, so it can fake them.
- A per-agent secret list was dropped: an agent that wants more simply would not use that client.
- Rules in a repository file can be edited by the agent; they became policy set in the app.
- Recipes, fork rules and keep hooks lived only in the CLI, so the engine could not enforce them.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/cli/src/folder.rs`
- `app/SiloUI/cli/tests/folder.rs`
- `docs/SiloUI-CLI.md#folder-forks`
- `docs/SiloUI-RECIPES.md#coming-from-silo`
- `docs/DIRECTION.md#L145`
- `docs/direction/PLAN.md#L346`

Calls: `silo fork ./dir`, `silo merge`, `silo undo ./dir`, `silo up`.
