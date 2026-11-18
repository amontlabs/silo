## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes becomes a line of `plan/decisions.json`.

- Silo builds in the loop and access and no orchestration; the user's orchestrator, scripts and subagents run inside a computer.
  - Recommendation: yes. Users already bring their own orchestrators, and task-extras is the one planned feature near the line.
- How does Silo integrate with another tool by default: (a) a skill the agent reads, or (b) adapter code for that tool?
  - Recommendation: talk it through. A skill costs nothing to write but is a prompt; an adapter suits a live link such as T3 Code or an editor.
- Silo has no messaging between agents; forks share no memory and pass work on only through files, the diff, the merge and a task's result.
  - Recommendation: yes. Agent programs bring their own messaging, and a fork's result can already be read.
- Support for a given stack, such as Rails or a mobile toolchain, is a recipe users fork and never a feature of Silo.
  - Recommendation: yes. Stacks have no end, and recipes already describe a computer.
- Silo has no board for tracking work, the owner's plan tooling included; other tools do it from Silo's events.
  - Recommendation: yes. A board is a product of its own, and the plan tooling already runs on GitHub.
