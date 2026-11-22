# How do we know every check ran

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

- Zig compiles only the code that is referenced, and runs only the tests it reaches from the test
  root, so a forgotten file never fails.
- Guillotine's integration test root has 40 of its 81 imports commented out, including
  `// CAUSES CRASH` and `// CAUSES INFINITE LOOP/HANG`
  ([test/root.zig](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/test/root.zig#L33)).
- Its spec runner can pass a case that checks no final state, and its snapshot test writes a
  missing baseline instead of failing. `check-must-catch-a-fault` says a check can catch a fault;
  this entry says it ran.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Checks run on both systems.** Run checks on every platform the code ships on, with the same
  diagnostics. The engine's checks run on macOS and Linux.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/scripts/agent-orchestrator/src/orchestrator.ts#L194))
- **Silent when everything passes.** A passing run prints nothing, so any output means a problem.
  `tools/ci/check.sh` prints a line per step today. Proposed: print only failures and a final line,
  and rely on the exit status.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/README.md?plain=1#L84))
- **Tests beside the code, suites as build steps.** Keep focused tests in the file they test, give
  each suite its own build step, and pass name filters through to the runner.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/preprocessor/dispatch_item.zig#L40))
- **Tests never wait for input, fixtures are embedded.** A test never reads standard input, and its
  fixtures are embedded at build time.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/TESTING.md?plain=1#L645))
- **Test the build itself.** Test the build configuration: its filters, its targets and their
  registration.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/build.zig.md?plain=1#L236))
- **Build steps check their tools and outputs.** Before running an outside tool the build checks it
  exists; generated outputs are verified before use, written to disjoint places, and removable by
  one step.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/build.zig.md?plain=1#L25))
