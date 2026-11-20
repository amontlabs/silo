# How the engine is cut into parts

```text
calls (app, command, agents) ──┐
driver results ────────────────┼─▶ one queue ─▶ core(state, input) ─▶ new state
clock ─────────────────────────┘                no disk, no network,   events  ─▶ clients follow
                                                no clock of its own    effects ─▶ drivers
```

- A call asks for a change. An event says what happened. An effect asks a driver.
- The meanings of the five words are worded from these lines; the record has no row for them in
  its list of words.
- The record uses "core" for two things: the engine itself, "the core piece"
  (`engine-language`), and the function inside it that decides. This decision gives the word to
  the second.
- What the engine does when a computer stops by itself is `computer-stops-by-itself`, in the
  part `computers`.

## The three ideas behind it

| Idea | Question it answers | What Silo takes |
|---|---|---|
| Data-oriented design | What is the state? | Flat tables, sizes known in advance. Not the tuning of memory layout: the engine tracks dozens of computers. |
| Event-driven design | How does state change? | Only through inputs, one at a time, each leaving a numbered event. Not a message broker: the engine is one program per device. |
| TigerStyle | How is the code written so it can be trusted? | Limits on everything, assertions, no allocation after start, plain control flow, no dependencies in the core, a simulator. In full in the core, where it fits in drivers. |

## Sources

- [TigerStyle](https://tigerstyle.dev/) and
  [TIGER_STYLE.md](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md):
  limits, assertions, no dependencies, a deterministic core.
