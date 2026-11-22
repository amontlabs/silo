# How do we know a fake driver behaves like the real one

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

- The simulator checks the core against fake drivers (`core-simulator`). A fake that drifts from
  its real driver makes the simulator check the core against a fiction.
- guillotine-mini gives each storage backend an in-memory version that implements the same
  interface, and runs shared tests against both.
