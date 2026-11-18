## Proposed

- Property: Nothing written inside a computer can change the record of what was done there, which is the engine's events and checkpoints, kept outside the computer.
- Property: No call, from a computer or from the app, edits an entry of the record or removes one.
- Rule: Git history inside a computer is content the agent controls and never serves as the record.
- Property: Every call that changed something is in the record with who made it, and the simulator checks it.
- Each action that was approved or that widened access leaves a receipt: what was approved, what was done with its exact inputs or a recording, and the result.
- For each agent action, the record shows the state it ran against: the computer, the checkpoint and what had changed since.

## Open

- Is the record of who did what kept whole, or capped per computer with the oldest rows dropped, as the old code did at 20,000 rows?
  - Suggested: Whole, with repeats collapsed and floods rate-limited, because under a cap an agent removes its own trail by making 20,000 new rows.
- Do any of Silo's records need a proof others can check, the audit journal for example, or is a plain copy enough?
  - Suggested: A plain copy for now, because there is one user for now, and a proof matters once someone else has to be convinced.

## From the old code

The old code had it built and driven end to end, about 3,000 lines, and its approach can be reused.

Lessons:
- Show plain actions, not method names (M finding 2).
- Per-computer row caps keep one computer from pushing out other rows.
- Refused handshakes are rate-limited to one row a second so a hostile program cannot flood the log.
- A computer cannot push out other callers' rows.
- Identical repeated rows collapse into one with a count, so a refusal loop cannot fill the log.
- Each computer has its own row share (20,000) apart from owner and device rows, so a busy computer cannot push them out.
- Activity first showed internal method names; plain words and hidden bookkeeping calls came after the live run.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/audit.rs`
- `app/SiloUI/src-tauri/silod/src/methods/audit.rs`
- `app/SiloUI/src-tauri/engine/src/audit.rs`
- `docs/direction/evidence/V.txt`
- `docs/direction/evidence/M.txt`
- `docs/SiloUI-POLICY.md#the-audit-log`
- `docs/SiloUI-POLICY.md#L183`
- `app/SiloUI/src/features/application/components/audit-log.tsx`
- `app/SiloUI/src/features/application/model/audit-copy.ts`
- `app/SiloUI/src/features/application/model/use-audit-log.ts`
- `docs/SiloUI-POLICY.md`
- `docs/ROADMAP.md#L331`

Calls: `audit.list`.
