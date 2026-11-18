## Proposed

- The engine is tested through the client the command line uses. Tests inside the Zig core cover only what that client cannot reach.
- Each component, the app included, can be run by an agent that reads its output, before features are built on it.

## Open

- Are the checks (lint rules, the simulator, the live-check harness) built before the first engine feature, or alongside? Checks first delay usable code; the plan's order has the simulator last in the shell stage.
- Is an engine feature built docs first (user docs, the interface, black-box tests, then code), or research first, then a plan, then code with its tests? Docs first give something to read early; building one feature each way would settle it.
