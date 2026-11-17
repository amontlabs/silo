## From the old code

The old code had it built, about 1,000 lines, and its approach can be reused.

Lessons:
- Lineage must be read through the replace-main journal, or forks of the old main point at the wrong computer.
- Being forked from a computer does not make it that computer's agent's: only computers it made are in its tree.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/cli/src/steps.rs`
- `docs/SiloUI-CHANGES.md#steps-and-the-fork-tree`
- `app/SiloUI/api/src/methods/steps.rs`
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints/steps.rs`
- `app/SiloUI/src-tauri/engine/src/steps.rs`
- `app/SiloUI/src-tauri/engine/src/computer_tree.rs`

Calls: `silo tree`, `computers.tree`, `steps.list`, `steps.record`.
