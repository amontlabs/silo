## Decided

- 2026-11-10, owner: a recipe is a property of a computer that Silo stores; its rules move into the app.

## Proposed, not confirmed by the owner

### For the owner to settle

- What is a recipe written in: a plain file such as TOML with shell setup lines and no Nix, a plain file with Nix underneath, or the Nix language?
  - Recommendation: A plain file with no Nix yet, because the plan keeps Nix for exact-rebuilds and any Nix a user can see may lose them.
- Can a recipe hold prompts for an agent to carry out or adapt to the project, or only fixed steps?
  - Recommendation: Only fixed steps, so that a rebuild gives the same computer; the agent's part is writing the recipe.

### For whoever builds it

- Format
  - Rule: Recipes and rules are written in an existing declarative format that people know and models already write correctly.
  - Rule: A notation of Silo's own is considered only once an existing language in a restricted mode has failed and agents measurably do better with it than with shell.
  - Rule: Recipes and rules are never written in YAML.
  - Detail: A recipe is made of small units that compose.
  - Docs: Before the first recipe format ships, the docs state where the declared part of a recipe ends and a script begins.
- What a recipe holds
  - Detail: A recipe names how the project runs and each service it starts, with its port and a check, so one command runs it in any fork.
  - Detail: From one source, a recipe generates the rules and skill files that the computer's agent program reads, such as a SKILL.md.
  - Detail: A recipe ships the usage notes of each agent tool it installs.
  - Detail: A recipe step can add reference repositories and the dependencies' source at their pinned versions, read-only and outside the checkout, for the agent to read offline.
- Build and apply
  - Rule: A recipe or rules file is evaluated in a restricted setting and returns only a plan, which the engine carries out.
  - Rule: Before it runs, anything an agent writes to drive Silo is checked for loop bounds, fan-out bounds, declared side effects and end states.
  - Detail: A setting refuses to run anything that did not pass that check.
  - Detail: Recipe steps are keyed by a hash of their inputs, so a changed step rebuilds only itself and what depends on it; the rest reuse their checkpoints.
  - To find out: Must a step name its inputs (files, a lockfile) for its hash to be right, or can Silo work them out and keep the recipe short?
  - Detail: One command drops a computer, rebuilds it from its recipe with no manual step, and reports what is missing afterwards.
  - Detail: A recipe change applied to a computer is reverted with one command.
  - Detail: A recipe unit can be tried in a fork before it is applied.
  - Rule: A failed recipe build is reported in Silo's words with what to do next, never as the raw error of the tool underneath, such as Nix.
- Shipped examples
  - Rule: A specific stack is covered by an example recipe and a skill, never by an integration built into Silo.
  - Rule: Shipped recipes, rules and Silo's skill hold only what an agent cannot infer and does not already know.
  - Rule: A shipped example recipe names no model, unless an automatic update keeps the name current.
  - The docs hold a few complete example recipes that together use every recipe feature.
  - The docs tell agents that they may edit the shipped example files.
- Reading and writing
  - Detail: Wherever a recipe or rules file is opened, the documentation of each field is at hand.
  - Test: A person who is not an engineer is given a recipe or rules file that an agent wrote; they read and approve it within a stated short time.
  - Test: A current model writes a recipe and a rules file, with the shipped docs pasted in and without; each is valid at the first attempt.
  - Test: That measure is taken again at each model release.
- Other
  - Detail: After a recipe is edited, Silo lists what must change in each computer built from it and offers to apply that without a rebuild.
  - Rule: The bundled recipes are kept apart from the files the project uses on itself, and every release builds and checks them.
  - Detail: Any recipe Silo ships is picked from the setups people already make most often.

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
