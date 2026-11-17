## From the old code

The old code had it only as an idea and it would be written again.

Lessons:
- Keeping a fork without asking when its tests passed was removed: the tests ran inside the fork, which the agent controls as root, so it could fake them.
- Anything an agent with root inside a computer reports about that computer (tests passed, tools OK) is its own claim and cannot gate a widening action.

Where it lived, at the tag legacy-2026-11-16:
- `docs/DIRECTION.md#L125`
- `docs/SiloUI-VOCABULARY.md#L64`
- `docs/direction/PLAN.md#L344`

Calls: `auto_keep`, `fold: auto-code`.
