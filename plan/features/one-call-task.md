## Proposed

- Rule: Before a task starts, the engine checks that its limits are set, its end condition is defined and the access it needs is already granted.
- A task declares its inputs and its result as schemas, and its result is never inferred from a file name.
- Rule: A task or child fork that succeeds but returns nothing its caller can see ends as an error.
- A task can name the agent program and the model it runs, so a planner, an implementer and a reviewer can each differ.
- The app fills a task's inputs from a small form built from its schema, so nobody writes JSON by hand.
- Find out whether a fast model that reviews a task and its prompt before it starts catches enough to be worth its cost and wait.
- One call can name several commands that run in one fork and land as one merge if all pass, or are dropped together.

## Notes

- Copy named result files out of the computer before it is removed.
- A handle for outside harnesses: start from a checkpoint, drive it (including screenshot, click, type), stop and discard.
- When macOS slots are full, wait or return a clear busy error.
- A timeout or crash leaves no orphaned computers or disks.

## From the old code

The old code planned it but did not build it and how much can be reused is not known.

Lessons:
- The pieces exist (fork with a command, merge --pr, review requests, the runner); what is missing is the layer that joins them into one unattended task.
- The CI runner was set aside on 2026-11-10 because its general form (something outside starts a task in a fresh fork and gets the result) comes first.

Where it lived, at the tag legacy-2026-11-16:
- `docs/product/JOURNEYS.md#L56`
- `docs/product/JOURNEYS.md`
- `docs/product/JOURNEYS.md#L58`
- `docs/DIRECTION.md#L203`

Calls: `silo fork main -- <cmd>`, `silo merge --pr`.
