## Decided

- 2026-11-16: a recipe sets up a whole working computer (agent, editor access, atelier, GUI automation), and making a recipe must be convenient.

## Proposed

- The guided questions are few, multiple-choice and about the user's goals, and the rules are picked from the answers.
- Silo ships a reference recipe that a user hands to their agent with one instruction to get exactly the same setup.
- A new recipe or a new set of rules runs step by step the first time and is automated afterwards.
- A scheduled check can rebuild main's recipe in a throwaway computer and report a recipe that no longer gives a working main.
- That check lists the secrets and environment variables that were not set after the rebuild.
- Making a recipe can start from a project's devcontainer, Dockerfile or other sandbox setup, and each path is tested on well-known open projects.

## Open

- Who guides the making of this recipe: the app with a few multiple-choice questions, or your agent adapting a reference recipe?
  - Suggested: Your agent, from a reference recipe, asking the few questions itself, because the agent lives inside and the app holds no logic.

## From the old code

The old code planned it but did not build it and how much can be reused is not known.

Lessons:
- Decided 2026-11-16: a recipe sets up a whole working computer, and making one must be convenient.
- The default Linux recipe has no Node, so most agent tooling needs setup before it runs.

Where it lived, at the tag legacy-2026-11-16:
- `docs/product/JOURNEYS.md`
- `docs/product/JOURNEYS.md#L34`
- `docs/product/DECISIONS.md`
- `docs/product/JOURNEYS.md#L33`

Calls: `silo recipe add`.
