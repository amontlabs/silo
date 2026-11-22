# How an operation of several steps survives a crash

The record checks `crash-finishes-or-rolls-back` with the simulator, with crashes.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Sibling operations fail the same way.** Audit related operations together, and give them the
  same failure handling. Checkpoint, restore, fork and drop answer the same failures in the same
  way.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/scripts/agent-orchestrator/reports/issue-842-iter-3-2025-12-21T06-02-52-444Z.md?plain=1#L27))
- **Keep the value from before.** Record the previous value before changing it, and keep the
  original, the current and the target when all three matter. Undo newest first and commit oldest
  first. A multi-step operation keeps what was there before it began, so a crash midway can be
  rolled back or reported.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/evm.zig#L268))
- **Created here versus before.** Track what was created in the current operation apart from what
  existed before it. `before-a-drop` lists what only a fork has. This is the same idea inside one
  operation: what it created is told apart from what was there, so a rollback removes only its own.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/storage/README.md?plain=1#L133))
