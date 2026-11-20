# How the core asks a driver to act

- The record's line reads "An effect is a request, not a call." "Request" is kept for what waits
  at the boundary, so the line is written "An effect asks a driver".
- The record checks `no-effect-twice` with the simulator, with crashes.
