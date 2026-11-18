## Proposed

- `silo resources` shows the memory, processor and disk each computer and fork uses, the total against the device's RAM and how many more forks fit, with no extra service to run.
- A fork that cannot start says which limit refused it, memory or disk, in the app and on the command line.
- The engine watches disk space, open files and process counts, for the device, each computer and itself, and warns the user or acts before one runs out.
- Property: A full disk on the device corrupts no computer; the engine stops what it must and says what is full.
- The memory estimate for a new fork counts the agent's own tooling, one language server per agent and its MCP servers, since every fork multiplies it.
- Rule: Sizing targets about 16 agents running at once on one device as the heavy case, and one or two forks as the common case.
- Test: An agent program that forks recursively without bound is refused at the floor; the device stays usable and the app says why.
- Test: Bring each watched limit close to its ceiling; a check fails before the limit is hit.
- Find out whether sizing and the floor must count a local model on the same device, for instance one that loads after forks have started.
- The docs say that a computer holds the memory it is given until it stops, because the guest uses free RAM as cache and the device cannot see inside.
- The resources view also shows the device's processor load and temperature while computers run long jobs.

## Open

- Does the app show resources in a pane of its own, in one total line in the window, or only through `silo resources`?
  - Suggested: One total line, since agents size forks from `silo resources` and `storage` later shows disk per computer.
- May a fork be given a disk cap as an option, when the notes record a decision for no hard caps and only a floor?
  - Suggested: Leave it out, since the floor and the disk watch already protect the device and a cap is one more setting per fork.
- To find out: Measure the memory a fork holds once typical agent tooling runs in it.

## From the old code

The old code had it partly built, about 3,300 lines, and its approach can be reused.

Lessons:
- Asking for approval on size was dropped: the agent sizes from what resources shows and the floor refuses.
- Decided: no hard caps on computers, only a floor that prevents freezing the device.
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
