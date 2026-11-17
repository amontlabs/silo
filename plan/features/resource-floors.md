## From the old code

The old code had it partly built, about 3,300 lines, and its approach can be reused.

Lessons:
- Asking for approval on size was dropped: the agent sizes from what resources shows and the floor refuses.
- The owner asked for no hard caps on computers; only a floor that prevents freezing the device.
- Admitted starts should reserve memory so a burst cannot pass the floor together (owed).
- The check and the hold must be under one lock, or concurrent approved starts slip past the floor.
- A burst of starts could each pass the floor before any computer took memory, so jobs reserve memory until the computer runs or 2 hours pass.
- Restart, undo and restore of a running computer keep its memory set aside, so they need no free memory.
- Open: whether the app shows a resource pane.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/device.rs`
- `app/SiloUI/src-tauri/engine/src/resources.rs`
- `app/SiloUI/src-tauri/engine/src/device_facts.rs`
- `docs/direction/evidence/V.txt`
- `docs/SiloUI-POLICY.md#limits`
- `docs/SiloUI-CLI.md#several-forks-and-commands`
- `app/SiloUI/src-tauri/engine/src/policy/resources.rs`
- `docs/SiloUI-POLICY.md#L203`
- `docs/DIRECTION.md#L222`
- `docs/direction/OWNER_REVIEW.md#L186`
- `docs/SiloUI-POLICY.md`
- `app/SiloUI/src/features/preferences/components/keep-free-section.tsx`

Calls: `device.facts`, `device.resources`, `device.dependencies`, `silo resources`.
