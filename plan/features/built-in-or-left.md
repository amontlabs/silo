## Proposed

- Silo builds in the loop and access, and no orchestration. The user's orchestrator, scripts and subagents run inside a computer.
- Silo has no messaging between agents. Forks share no memory and pass work on only through files, the diff, the merge and a task's result.
- Support for a stack, such as Rails or a mobile toolchain, is a recipe users fork, never a feature of Silo.
- Silo has no board for tracking work. Other tools build one from Silo's events.

## Open

- How does Silo integrate with another tool by default: a skill the agent reads, or adapter code for that tool? A skill costs nothing to write but is a prompt; an adapter suits a live link, such as T3 Code or an editor.
