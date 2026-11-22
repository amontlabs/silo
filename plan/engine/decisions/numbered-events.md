# How does a client follow the engine

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **A ring checks the number it hands out.** When a fixed-size ring wraps, verify the full key or
  number on every read, and keep the wrap arithmetic inside the ring. A client that asks for the
  events after N, when the first one after N has already left the ring, is told to resync, never
  handed later events silently.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/BUG_HUNT_REPORT.md?plain=1#L411))
