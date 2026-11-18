## Proposed

- Property: The checks that decide a fork's merge are defined outside the fork, run by the engine against its result, and cannot be edited by its agent.
- A task or fork carries acceptance commands, such as build and test, as its end condition, and reports "done" or "not done" against them.
- A recipe defines the project's checks once, and they run in every fork on each commit and before it asks to merge.
- When a check fails, the engine writes the reason to a file in the fork for its agent to read.
- A fork's diff shows the result of its checks.
- The engine records when a fork's checks first passed.
- A check can name the paths it covers and runs in the fork, with no push, whenever one of them changes.
- A check can run in a fork with the network closed, and its record shows the network stayed closed for the whole run.
- Rule: Silo checks what a fork did by running it again or by comparing state, and it offers no cryptographic proof.

## Open

- When a fork's checks fail, does its merge request still reach you with the failure shown, or wait until they pass?
  - Suggested: An agent's merge request waits until they pass, and you can still merge a failing fork yourself, since merges are your call.
- Can the agent in a fork read the checks that decide its merge, or only see whether they passed?
  - Suggested: Read them, since they are mostly the project's own tests in the repository, and the run in a fresh fork already stops a faked pass.

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
