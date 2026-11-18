## Proposed, not confirmed by the owner

### For whoever builds it

- To find out: Run a Rust build in a computer with 16 GB of memory, record whether it runs out, and state a minimum size for Rust work.

## From the old code

The old code had it built, about 1,500 lines, and it needs reworking.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/computers.rs`
- `app/SiloUI/src-tauri/silod/src/methods/computers.rs`
- `app/SiloUI/src-tauri/engine/src/runtime`

Calls: `computers.configuration.change`, `computers.configuration.read`, `computers.configuration.retry`, `activity.setup.read`.
