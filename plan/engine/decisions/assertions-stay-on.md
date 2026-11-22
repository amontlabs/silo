# Do assertions stay on in release builds

The record adds that computers outliving the engine (`when-the-engine-stops`) makes this
acceptable.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

The answer's last sentence was added on 22 November 2026, from these lessons.

- **A stop is a stop in every build.** A fatal path ends in a defined panic in every build mode,
  never in `unreachable`, which is undefined behaviour in optimized builds. Zig's `std.debug.assert`
  and `unreachable` do not stop in ReleaseFast or ReleaseSmall: the optimizer assumes they never
  happen. Build the engine in ReleaseSafe only, and have the test that reads the core's source
  refuse a bare `unreachable`, as `engine-states-and-failures` (decided) already refuses `catch
  unreachable`.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/tracer/tracer.zig#L676))
- **The checked build is the shipped build.** Checks and limits behave the same in every build. Only
  optional diagnostics, such as traces and dumps, may be switched off, and no setting can switch off
  a check in a release build. Guillotine compiles its checks out of its fast build and turns its
  instruction limit off there, so what it tests is not what it ships. In Silo the check script runs
  the build that ships.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/frame/frame.zig#L393))
- **Checks never wait for an observer.** An invariant check that guards an unchecked step runs
  whether or not a tracer is attached. No assertion in the core sits behind a tracing or debug
  switch.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/stack/stack.zig#L88))
- **Print recent history before stopping.** Before a fatal stop, print a bounded window of the most
  recent steps next to the message. The ring belongs to the loop that feeds the core, not to the
  core, which keeps nothing between calls. When the engine stops on an assertion, that loop prints
  the last inputs and effects as bounded metadata (kind, id, computer, sizes), never secrets or
  content. In the simulator the stop replays from the seed; in production the ring is the evidence.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/tracer/tracer.zig#L665))
