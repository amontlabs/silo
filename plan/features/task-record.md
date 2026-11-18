## Proposed, not confirmed by the owner

### For the owner to settle

- Does a task's record also keep the agent's plan, notes on what failed and a hand-off note, outside the computer so a restore cannot erase them?
  - Recommendation: Yes, because you or another agent can then take over a fork after a restart or a restore without losing what was learned.

### For whoever builds it

- Property: A task's record and a status query always show the current run and its real state, and never an earlier failed run or a stale value.
- Detail: Each fork records the model, the effort setting, the agent program and its version, and the account that paid, per session.
- Detail: A task's record keeps its number of attempts, the checks that failed and whether its fork was merged or dropped.
- Detail: A fork keeps the request that started it word for word, and every task delegated from the fork links back to it.
- Detail: A task's timings split active work from waiting on tests, builds and the model, so setup that every run repeats shows up.
- Detail: A task's record can be exported as a file.
- Test: Repair or resume a run, then check that its timestamps are still correct.
- Test: Let an agent in a computer start nested workflows of its own; Silo still shows the task's status and timings correctly.
- Find out whether ATIF, Harbor's trajectory format, can be the record of an agent session in a fork before Silo defines its own.

## From the old code

The old code had it only as an idea and how much can be reused is not known.

Where it lived, at the tag legacy-2026-11-16:
- `docs/product/JOURNEYS.md#L58`
