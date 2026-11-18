## Proposed, not confirmed by the owner

### For the owner to settle

- Does a fork sit under the computer it was forked from, as planned, or under whoever made it, with the computers you made at the top?
  - Recommendation: Under the computer it was forked from, because a merge goes there; the row can show who made it.
- Does the first tree also get a zoomable map of forks with their live output, a timeline scrubber over checkpoints and events, both, or neither?
  - Recommendation: Neither yet: the plan takes the tree as small, and `pages-are-short` asks for one place to look.

### For whoever builds it

- Rows and states
  - Detail: The tree is collapsible and groups forks from the start, so the list never grows by one flat row per fork.
  - Detail: Each row shows the fork's name, its state and what it is for, with the original request one step away.
  - Property: Running, waiting for approval, exited and failed are four distinct states, shown for every agent without opening its process.
  - Detail: A fork waiting on the user shows "waiting for you" and how long it has waited.
  - Property: A child fork's pending question or wait for a decision never marks its parent as failed.
- Grouping
  - Rule: The user can always see and choose how forks are grouped, and a grouping an agent curated is never the only view.
  - Detail: One view lists every fork working on one repository.
- What a fork records
  - Detail: Each fork and each checkpoint records the prompt or session that led to it and a short description written by the agent.
  - Detail: Each fork records the open question or assumption it was built on, so the forks to redo after an answer can be listed.
  - Detail: A fork's record carries the plan and the research behind it.
  - Detail: Where a fork belongs to a task, its record names the task with its status, and merging the fork closes the task.
- What an agent sets
  - Detail: An agent can set a status line, a title and notes on the rows of its forks through calls, and the app shows them.
  - Rule: A status line says what was checked and when, what is not confirmed, and what has not been done.
  - Detail: Fork and session names are generated knowing the other recent names, so the names show how forks relate.

## From the old code

The old code had it built, about 1,000 lines, and its approach can be reused.

Lessons:
- Lineage must be read through the replace-main journal, or forks of the old main point at the wrong computer.
- Being forked from a computer does not make it that computer's agent's: only computers it made are in its tree.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/cli/src/steps.rs`
- `docs/SiloUI-CHANGES.md#steps-and-the-fork-tree`
- `app/SiloUI/api/src/methods/steps.rs`
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints/steps.rs`
- `app/SiloUI/src-tauri/engine/src/steps.rs`
- `app/SiloUI/src-tauri/engine/src/computer_tree.rs`

Calls: `silo tree`, `computers.tree`, `steps.list`, `steps.record`.
