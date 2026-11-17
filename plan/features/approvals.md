## Decided

- 2026-11-10, owner: approvals are native to Silo: shown in the Silo app, two-way with the agent, not a prompt of the agent's harness. No broker agent.

## From the old code

The old code had it built and driven end to end, about 7,500 lines, and its approach can be reused.

Lessons:
- The summary must come from the call's parameters, never from the agent's text.
- Approvals are answered on the device whose computer asked.
- Write the summary from the call's parameters, never the agent's words, and strip control characters and direction overrides from anything the agent chose.
- An allowed call is checked again as if just made, then run from the stored copy.
- Limits: 30 an hour, 20 waiting, seven-day expiry; a secret value is removed from the file when the approval ends.
- Summaries are written by the engine from parameters, never from the agent's words; agent text is stripped of control and direction characters.
- An allow is recorded first and never re-run on its own after a crash.
- Deny on a merge approval must close the review request too, or it comes back as its own row.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/approvals.rs`
- `app/SiloUI/src-tauri/silod/src/methods/approvals.rs`
- `app/SiloUI/src-tauri/silod/src/methods/approvals/tests/approvals_mbt.rs`
- `docs/direction/evidence/V.txt`
- `docs/SiloUI-POLICY.md#L92`
- `app/SiloUI/src-tauri/engine/src/approvals.rs`
- `app/SiloUI/src-tauri/engine/src/policy/summary.rs`
- `app/SiloUI/cli/src/approvals.rs`
- `docs/SiloUI-POLICY.md#asking-you`
- `docs/direction/evidence/T5a.txt`
- `docs/SiloUI-CLI.md#approvals`
- `app/SiloUI/cli/tests/approvals_wait.rs`

Calls: `approvals.list`, `approvals.get`, `approvals.decide`, `approvals.cancel`, `approvals.note`, `notifications.list`, `silo approvals`, `silo approvals show`, `silo approvals allow`, `silo approvals deny`, `silo approvals status`, `silo approvals wait`, `silo approvals note`, `changes.requests.*`.
