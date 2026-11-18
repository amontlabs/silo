## Proposed

- The core has no dependencies. The drivers around it hold every library.
- The core is deterministic, so a simulator run replays exactly from its seed.
- Every input of every call has a stated maximum size, and a check fails on a call without one.
- Product code is Zig and Swift, meeting at one C interface. Another language enters only by a decision.
- Every Linux build, of the engine and of the `silo` command inside a computer, is cross-compiled from the Mac with Zig, for x86_64 and aarch64.
- A platform (macOS, Linux on x86_64 or aarch64) counts as supported once the whole check runs on it.

## Open

- Does the core allocate its memory at start, with a fixed maximum for every table? Limits and simulation become exact, but the plan lets agents create computers until memory or disk runs out.
- Does the engine follow TigerBeetle's TigerStyle, named in one line of the rules? It includes the static allocation above.
