## Proposed, not confirmed by the owner

### For the owner to settle

- What memory does a new computer get by default: 8 GB, the edge for a large Rust build, or 16 GB, which is safe?
  - Recommendation: 16 GB, because a machine too small for integration and browser tests is the recurring complaint about sandboxes.

### For whoever builds it

- Property: Creating a computer and setting its rules are one change, so a computer never exists without its rules.
- Property: Stopping or deleting a computer, or dropping a fork, ends every process started in it, detached ones included.
- Detail: Each computer has CPU and memory limits, so tests in one fork do not starve the others.
- Property: A computer that runs out of memory stops only itself.
- Property: A computer has no time limit, and nothing stops it after 24 hours.
- Detail: Computers and operations carry a readable name and a description beside their id.
- The docs give the reason for each default size: 8 GB is the edge for a large Rust build, 16 GB is safe, and a large Lean proof can exceed 16 GB.
- The docs say plainly whether a computer gets a GPU.

## From the old code

The old code had it partly built, about 9,800 lines, and its approach can be reused.

Lessons:
- A computer cannot stop or delete itself; an agent may only stop or delete computers it made.
- A running fork must be stopped before it can be dropped (M E3), which agents trip over.
- A computer cannot stop or delete itself; an agent works only on computers it made.
- M: a macOS computer made with `silo create --macos` never appeared in the app's list.
- A running fork must be stopped before it can be dropped (journey 2 gap).
- On macOS the account group is staff, which every account shares; tokens must be 0600 owner-only there.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/silod/src/methods/computers.rs`
- `app/SiloUI/api/src/methods/computers.rs`
- `docs/direction/evidence/M.txt`
- `docs/direction/evidence/V.txt`
- `docs/SiloUI-POLICY.md`
- `app/SiloUI/cli/src/cli.rs`
- `docs/SiloUI-CLI.md#commands`
- `app/SiloUI/src-tauri/guest/working-account.py`
- `app/SiloUI/src-tauri/guest/working-account.sh`
- `app/SiloUI/src-tauri/engine/src/working_account.rs`
- `docs/SiloUI-WORKING-ACCOUNT.md`
- `app/SiloUI/scripts/test-working-account-live.py`

Calls: `computers.create`, `computers.start`, `computers.stop`, `computers.restart`, `computers.forceStop`, `computers.delete`, `computers.dismissError`, `computers.list`, `silo create`, `silo start`, `silo stop`, `silo retry-setup`, `silo drop`.
