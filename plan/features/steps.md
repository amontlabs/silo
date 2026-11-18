## Proposed, not confirmed by the owner

### For the owner to settle

- Should a fork's timeline of checkpoints, commands and approvals come early, in the starter, when steps is planned after it?
  - Recommendation: Keep it after the starter, because the diff already shows what a fork changed and the timeline matters once runs are long.

### For whoever builds it

- Checkpoints along the way
  - Property: For every step of a computer, a checkpoint from before it exists, so any step can be reverted.
  - Detail: The engine takes checkpoints on its own while an agent works, without the agent asking and without adding anything to the repository's history.
  - Detail: A loop running in a fork gets a checkpoint before and after each iteration.
  - Detail: Silo takes a checkpoint at every question an agent asks, so a different answer later forks from there and runs again.
- The record
  - Rule: The record of the commands run in a fork and of their exit codes is kept by Silo and cannot be written by the agent.
  - Detail: A merge can require that record.
  - Detail: One history holds what the agent did and what the person did.
  - Detail: Each step shows the agent, the login and the model that did it.
- The history view
  - Detail: A fork has a short clickable timeline of its checkpoints, commands and approvals, and a click shows the computer's state at that step.
  - Detail: A frame-by-frame view with events is one step away from the timeline, for debugging.
  - Detail: The history collapses a routine stretch into one line with a count and a summary, and keeps decisions and churn visible between them.
  - Detail: The history is one vertical list with zoom levels.
  - Detail: Each stretch of a session is labelled orienting, thrashing or progress, from the exit codes and read/write ratios the engine already records.
  - To find out: Do plain rules label mixed debugging stretches well enough, or does it take a small model?
  - Detail: Tokens and cache use are shown per stretch of the history.

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
