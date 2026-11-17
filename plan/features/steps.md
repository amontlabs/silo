## From the old code

The old code had it partly built, about 800 lines, and its approach can be reused.

Lessons:
- Command lines are stored as typed, so secrets in arguments end up in the step list.
- A step is a command run through Silo; edits over plain SSH are not seen.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/cli/src/steps.rs`
- `app/SiloUI/cli/tests/steps.rs`
- `docs/SiloUI-CHANGES.md#steps-and-the-fork-tree`
- `app/SiloUI/src/features/application/components/steps-panel.tsx`
- `app/SiloUI/src-tauri/src/commands/review_requests.rs`
- `docs/v1/evidence/phase-7/app-check.txt`
- `docs/v1/BUILD_LOG.md`

Calls: `silo steps`, `silo fork --from-step`, `silo exec --step-checkpoints`, `steps.record`, `steps.list`, `silo tree`.
