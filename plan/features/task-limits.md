## Proposed, not confirmed by the owner

### For whoever builds it

- Rule: The engine enforces a task's limits from outside the agent program, so they hold whatever the agent program is set to.
- Rule: Nothing that stops or reclaims a fork treats "no work right now" as "finished".
- Property: With a limit of N running forks or tasks, a finished one frees its slot at once, and the next waiting one starts without waiting for the rest.
- Detail: Besides time, a task can be limited in iterations and in retries.
- Detail: A fork's time, cost and retry limits can be changed while it runs.
- Detail: When no default limits are set, the first task to start asks for them once, and settings keep the answer.

## From the old code

The old code had it only as an idea and how much can be reused is not known.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-POLICY.md`
- `docs/direction/OWNER_REVIEW.md#L186`
