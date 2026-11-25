# How does a client follow the engine

## The choice

An agent's part, 22 November 2026. The app is open while three forks start, and your Mac sleeps for
a minute in the middle. The app missed part of what happened.

The options:

- Numbered changes (recommended). The engine numbers every change. The app holds the state as of one
  number and applies each change after it, in order. After the sleep it asks for what it missed, or
  reloads everything when it missed too much. The app and the command line then only ever show a
  state the engine really had. It costs a short reload now and then.
- Ask for everything, every so often. The app asks the engine for the whole state on a timer. It is
  simpler. It costs lateness, and what came and went between two asks is never shown, such as a fork
  that started and died.

Recommended: numbered changes. The app is where you read what your agents did, so it must never show
something that was not so.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **A ring checks the number it hands out.** When a fixed-size ring wraps, verify the full key or
  number on every read, and keep the wrap arithmetic inside the ring. A client that asks for the
  events after N, when the first one after N has already left the ring, is told to resync, never
  handed later events silently.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/BUG_HUNT_REPORT.md?plain=1#L411))

## The first build's spec

Agent, 24 November 2026, from [the build specs of the first
build](../../../research/2026-11-24-first-build-specs/README.md). The specs put a history id beside
the number, as #352 (file 04) proposes: in the engine's saves, in `welcome`, and in each `follow`.
The engine's history is also its count in the record, so a line's source says which history and
which event it came from. Without the history id, the field goes and a client that missed a rebuild
can show a state the engine no longer has.
