## Decided

- 2026-11-16, orchestrator at the owner's word ("go as you think is cleanest"): merging a task into main changes main only. It does not write the merged packages and files into the saved recipe; a recipe changes when the owner says so.

## Proposed, not confirmed by the owner

### For whoever builds it

- Detail: The diff lists every package and tool a fork installed and the setup commands it ran.
- Detail: Each of them can be proposed as a recipe entry, which the user accepts or declines.
- Detail: Main's page lists how main now differs from its recipe, and you pick which differences go into the recipe.

## Notes

- Merging a package or setting can append it to the recipe, and the recipe change can go out as an ordinary pull request so teammates get it on their next build.

## From the old code

The old code planned it but did not build it, about 500 lines, and it needs reworking.

Lessons:
- An agent's merge must never add packages that the owner's next rebuild installs on every computer from that recipe.
- An agent's merge must change only that computer's record, never the owner's saved recipe.
- A merge an agent ran changes the saved recipe only with the owner's approval, so recipes can fall behind.

Where it lived, at the tag legacy-2026-11-16:
- `docs/product/DECISIONS.md`
- `docs/direction/OWNER_REVIEW.md#L9`
- `docs/SiloUI-RECIPES.md#what-a-computer-remembers`
- `app/SiloUI/src-tauri/engine/src/recipes/`
- `app/SiloUI/src-tauri/engine/src/recipes/additions.rs`
- `docs/SiloUI-RECIPES.md`

Calls: `changes.keep`.
