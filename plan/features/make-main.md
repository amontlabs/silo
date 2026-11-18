## Proposed

- What it does
  - Rule: First run ends with a working setup already chosen, main, a recipe and sensible rules, that works after one command; the user edits it afterwards.
  - Rule: The common case, one agent and one fork, asks for nothing beyond signing in and choosing an agent program.
  - Rule: First run sets up only what will work, and asks where it would otherwise guess.
  - The guided first loop, if taken, goes one step at a time with the user approving each, takes minutes and needs no documentation.
  - First run ends by showing what was set up.
  - The wait for the first image download and the first boot shows something worth looking at, more than a spinner.
- What it never does
  - Rule: First run installs nothing the user did not ask for, examples and skills included, and nothing outside the place the user named.
  - Rule: If examples are offered, the user picks which.
  - Rule: Setup never depends on an interactive wizard in a terminal; anything that needs the user goes through the app.
  - Rule: No install step and no permission prompt appears before the user has signed in or asked for it.
- Budget
  - Property: From install to a first fork merged or dropped on a small task, the time and the number of inputs stay within a budget.
  - The starting target is one pasted command, an interface within ten seconds, and a first task running after at most four inputs.
  - Test: That budget is timed on a clean Mac at each release, beside the fork, start and diff budgets, and driven down.
  - Test: Count how many first runs reach a running first task within that target.
  - Risk: An agent-led first run can use up a small subscription, so what the agent's part spends is measured and kept well under a $20 plan's weekly limit.
- Tests
  - Test: The test of first run drives the common case, one agent and one fork, to a working setup.
  - Test: For each combination of connected agent programs, setup produces the expected result, and an agent program signed in against another provider is detected as such.
  - Test: Each version of first run, its question step above all, is tried on a few outside users before it is kept.
  - Test: At every step where the user picks something, drive first run with no selection, a wrong selection and a slow one; none leaves it stuck.
  - Test: Run the part of first run that an agent performs with each main model, and check it from the engine's event log.
  - Test: Whatever first run generates, such as a recipe or rules, is checked to be valid.
  - Test: The first health check of a new setup runs one full loop, a fork, work in it and a merge into main, and ends healthy.

## Open

- Does first run go on to a guided first fork, diff and merge or drop on a sample task, or stop once main is ready?
  - Suggested: Offer it as a step the user can skip, since the loop is the product and nothing else shows it in the first minutes.
- Is first run led by the app, step by step, or, after one question, by the user's own agent inside main as a conversation?
  - Suggested: The app, with the agent's session offered at the end, since an agent-led setup takes control from the user and spends their subscription.

## From the old code

The old code had it partly built, about 3,400 lines, and its approach can be reused.

Lessons:
- A Dev app can open on 'Silo could not load' after an older engine left its record (journey 1).
- The dependency check screen was driven in a signed bundle on 2026-09-08; the full first run in today's app is not driven.
- Setup must wait for real computer and GitHub acknowledgements; an earlier prototype faked progress with fixtures.
- A removed runtime file was reported by exact name and Continue stayed disabled; keep that failure path tested.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-RECIPES.md#silo-up`
- `docs/product/JOURNEYS.md`
- `app/SiloUI/cli/src/ops.rs`
- `app/SiloUI/src/features/onboarding/onboarding-app.tsx`
- `app/SiloUI/src/features/onboarding/steps`
- `app/SiloUI/src/desktop/production-onboarding.tsx`
- `docs/SiloUI-DEPENDENCIES-BACKUP-TESTING.md#L199`
- `docs/product/JOURNEYS.md#L22`
- `app/SiloUI/src/features/onboarding/steps/dependencies-step.tsx`
- `app/SiloUI/src/desktop/dependencies.ts`
- `app/SiloUI/src-tauri/src/commands/dependencies.rs`

Calls: `silo up`, `agents.install`.
