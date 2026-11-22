# How it is checked

How Silo shows that a property holds.

## For

To show that what Silo promises always holds (`properties-are-checked`).

## Why it is so

- **Properties are stated and checked while Silo is built**, not added once the code exists
  (`properties-are-checked`).
- **Speed is a requirement** (`observing-speed`), so the budget of fork, start, checkpoint and
  idle memory is written before their code (`speed-budgets`) and measured in a way a laptop cannot fool
  (`how-speed-is-measured`). Where the time goes is said in `core-is-not-the-slow-part`.
- The ways of checking Silo uses, and where, are settled in `model-of-access-first`, `model-tool`
  and `four-ways-to-check`. How the core is simulated is not settled yet: `core-simulator`.

## One example

Forking a computer gets its speed target before the code that forks is written
(`speed-budgets`). The code is then measured against it by many interleaved runs
(`how-speed-is-measured`).
