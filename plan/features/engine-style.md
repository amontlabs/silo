## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- The engine follows TigerBeetle's TigerStyle, named in one line of the rules and not restated there. (`AGENTS.md`)
  - Recommendation: talk it through. A public style is cheap to name, but it brings static allocation, which the next line weighs.
- The engine core allocates its memory at start, with a fixed maximum for every table. (`AGENTS.md`)
  - Recommendation: talk it through. It makes limits and simulation exact, but resource-floors lets agents make computers until memory or disk runs out.
- Decision engine-in-zig writes the whole engine in Zig; instead, the drivers, the long-running service and any helper tools are written in Go, which agents write better. (`plan/decisions.json`)
  - Recommendation: no. Settled on 16 November, and a second language widens the one boundary the rules keep.
- The engine core has no dependencies, and the drivers around it hold every library. (`AGENTS.md`)
  - Recommendation: yes. It keeps the core small enough to read and to simulate.
- The engine core is deterministic, so a simulator run replays exactly from its seed. (`AGENTS.md`)
  - Recommendation: yes. The simulator of properties-checked depends on it, and it is nearly free if decided before the first line.
- Every input of every call has a stated maximum size, and a check fails on a call without one. (a check in `tools/ci/check.sh`)
  - Recommendation: yes. With every call defined once (calls-and-events), the check reads one file.
- Silo's product code is Zig and Swift, meeting at one C interface, and another language enters only by a decision. (`plan/decisions.json`)
  - Recommendation: yes. Every extra language is a boundary agents get wrong and the owner must read.
- Every Linux build, of the engine and of the `silo` command inside a computer, is cross-compiled from the Mac with Zig, for x86_64 and aarch64. (`AGENTS.md`)
  - Recommendation: yes. Zig does it out of the box, so one Mac builds every target.
- A platform (macOS, Linux on x86_64 or aarch64) counts as supported only once the whole check runs on it. (`AGENTS.md`)
  - Recommendation: yes. atelier already runs the check per platform; this says when a platform may be claimed.
