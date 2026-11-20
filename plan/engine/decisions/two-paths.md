# Do facts and reports take the same path as decisions

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
