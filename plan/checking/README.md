# How it is checked

How Silo shows that a property holds: assertions, the simulator, the model of access, hostile
tests, measurements.

## For

To show that what Silo promises always holds (`properties-are-checked`).

## Why it is so

- **Properties are stated and checked while Silo is built**, not added once the code exists
  (`properties-are-checked`).
- **Speed is a requirement** (`observing-speed`), so each operation's target is written
  before its code (`speed-budgets`) and measured in a way a laptop cannot fool
  (`how-speed-is-measured`). Where the time goes is said in `core-is-not-the-slow-part`.
- Which ways of checking Silo uses, and where, are not settled yet: `model-of-access-first`,
  `model-tool`, `four-ways-to-check`, `core-simulator`.

## One example

Forking a computer gets its speed target before the code that forks is written
(`speed-budgets`). The code is then measured against it by many interleaved runs, in a computer
and on a plain machine (`how-speed-is-measured`).
