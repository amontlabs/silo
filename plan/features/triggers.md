## Proposed, not confirmed by the owner

### For the owner to settle

- Besides GitHub events, schedules and Fjord, can other events start a task: a webhook, an alert, a run starting, something you did by hand?
  - Recommendation: Later, through the same trigger shape, since a trigger only calls "start a task" and each new source then costs little.

### For whoever builds it

- Rule: The fix from a task that an outside event started waits for a person, who sees the event's text beside the diff to check it for prompt injection.
- Detail: A ticket a person approved, such as a GitHub issue given a label, starts one fork that carries the ticket's id.
- Detail: When a fork started from a ticket merges, the task reports back on the ticket.
- Detail: A fork started from a ticket takes a checkpoint at each commit.
- Detail: A schedule can target an existing fork, such as "every ten minutes, report or fix", and is set up without a script.
- Rule: Text from the issue, comment or log that starts a task reaches the agent as untrusted input, and the task's access comes from the trigger's rule alone.

## From the old code

The old code planned it but did not build it and how much can be reused is not known.

Lessons:
- Updating main after a pull request merges is an ordinary git pull, not a Silo feature (owner, 2026-11-16).

Where it lived, at the tag legacy-2026-11-16:
- `docs/product/JOURNEYS.md#L65`
- `docs/product/DECISIONS.md`
