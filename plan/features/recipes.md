## Decided

- 2026-11-10, owner: a recipe is a property of a computer that Silo stores; its rules move into the app.

## Notes

- Old recipe fields: base, packages, setup, services, quiet.
- Bundled recipes: `linux-dev`, `macos-clean`, `macos-xcode`.
- `silo import` brought in existing Tart and Lume machines.

## From the old code

The old code had it built and driven end to end, about 5,000 lines, and its approach can be reused.

Lessons:
- A computer reads only the recipe it was built from (V I2b).
- Any computer may fork the hidden silo-build- computer of a recipe, and gets nothing beyond what the caller has.
- Package names and service commands reach build scripts only as environment variables, never as shell text.
- A computer never sees a recipe's command lines, because the owner may have pasted a token there.
- A rebuild keeps the old cache until the new build succeeds.
- A macOS build needs one of the two macOS VM slots.
- Package names are passed as arguments, never shell text; setup lines are shell text.
- Tokens never go in setup or test lines: every computer built from it can read them.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/recipes.rs`
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints/recipes.rs`
- `app/SiloUI/src-tauri/engine/src/recipes`
- `docs/SiloUI-RECIPES.md`
- `docs/product/JOURNEYS.md#L54`
- `docs/product/JOURNEYS.md#L56`
- `app/SiloUI/src-tauri/engine/src/recipes/build.rs`
- `app/SiloUI/src-tauri/engine/src/recipes/library.rs`
- `app/SiloUI/src-tauri/engine/src/recipes/records.rs`
- `docs/SiloUI-RECIPES.md#build-and-cache`
- `docs/product/JOURNEYS.md`
- `app/SiloUI/src-tauri/engine/src/recipes/`

Calls: `recipes.save`, `recipes.get`, `recipes.list`, `recipes.remove`, `recipes.build`, `computers.recipe`, `silo recipe`, `silo up`, `silo recipe list`, `silo recipe build`, `silo recipe remove`, `silo fork --recipe`, `silo recipe add`, `silo recipe show`.
