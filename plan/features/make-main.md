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
