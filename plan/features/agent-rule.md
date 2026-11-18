## Proposed

- Property: No call from inside a computer changes that computer's own rules or its own recipe without an approval.
- Property: By default, no code or configuration written by an agent takes effect on a running engine without an act by the user.
- Rule: Rules hold for the whole computer, so an agent that another agent starts through the shell gets nothing more.
- The docs say that an agent can create accounts on any outside service its network rules let it reach, and that Silo does not show them.

## Open

- Needs `approvals`, which comes later in the order, so that an agent can ask instead of only being refused.

## From the old code

The old code had it built and driven end to end, about 13,300 lines, and it needs reworking.

Lessons:
- A new method must not compile until it is classified for the computer caller.
- Lists that cover every computer (operations, events, notifications) were refused to agents because they were not cut per caller yet.
- 'Made by', not lineage, decides reach: a fork the user made by hand may hold more than its parent.
- After replace main the made-by records must move, or the old main keeps control of the new one.
- A computer with no saved network policy must be judged closed, not open (imports).
- Any same-user process on the device counts as the user today; programs on the device have no identity yet.
- A fork you made by hand from main may hold more than main, so lineage is not authority.
- What a computer may touch is decided by a recorded created_by, not by lineage: a fork you made by hand may hold more access than its parent.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/silod/src/methods/gate.rs`
- `app/SiloUI/src-tauri/silod/src/methods/access.rs`
- `app/SiloUI/src-tauri/engine/src/policy`
- `docs/SiloUI-POLICY.md`
- `docs/direction/evidence/V.txt`
- `app/SiloUI/src-tauri/engine/src/policy/rules.rs`
- `docs/direction/evidence/T5a.txt`
- `docs/direction/evidence/M.txt`
- `app/SiloUI/src-tauri/engine/src/policy/mod.rs`
- `docs/THREAT-MODEL.md#L741`
- `app/SiloUI/src-tauri/engine/src/policy/store.rs`
- `app/SiloUI/src-tauri/engine/src/policy/handoff.rs`

Calls: `computers.access`, `methods::dispatch`, `approvals.list`, `approvals.decide`.
