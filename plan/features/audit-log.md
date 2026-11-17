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
