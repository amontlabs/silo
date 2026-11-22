# The record path and the core

## For the owner

An agent's part, 22 November 2026. A build changes 50,000 files in a minute in main. During that
minute you take a secret back from main in the app.

The options:

- The record has a path of its own (recommended). The 50,000 lines go straight to the record, and
  your click is handled at once. It costs a record that can be a moment behind what the app shows.
- One path for everything. Every line passes where your click is handled. It costs your click
  waiting behind the 50,000 lines.

Recommended: a path of its own. What you do must never wait behind what an agent's build produces.

## Notes

"Core" is the engine's word, set by `engine-shape` in the part `engine`. The record names no check
for this sentence: `types` is put forward here.

```text
 control path
 calls, driver results, clock ─▶ core ─▶ state, events, effects ─▶ drivers
                                           │
                                           └─ every event is also written as a control fact
 record path                                         ▼
 drivers at the boundary ─▶ facts ──────────┬─▶ the record ─▶ classifiers ─▶ opinions
 programs inside the computer ─▶ reports ───┘        │                          │
                                                     └──── queries ─────────────┴─▶ app, command, added classifiers
```

## Why two

The core handles a few inputs a second for dozens of computers. A build produces file changes by
the tens of thousands. Facts and reports at that volume must not pass through the core, and the
core must never wait for them.

The principles of the record path are in the part `record`.

## Sources

- [Turning the database inside-out](https://martin.kleppmann.com/2015/03/04/turning-the-database-inside-out.html)
  (Martin Kleppmann): a log of facts as the primary thing, and every other structure as a view
  derived from it. This is the ground under "the record, then opinions".
