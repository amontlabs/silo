## Proposed

Why: With several forks running, you open each one or ask its agent to learn what it is doing.

- Rule: Progress comes from observed facts, such as checkpoints, checks passed and time since the last change, and a model's own estimate is never shown as fact.
- Rule: The watch runs in the engine, spends no tokens and depends on nothing inside the fork.
- A fork that keeps running while its diff and its checks stay the same is flagged as not progressing.
- An agent that is idle, or finished with its process still running, is told apart from one that works.
- The line says why an agent stopped: a spent quota with its reset time, a provider that is down, a model that refused, or a failed task.
- A fork that becomes stuck or waits for quota sends a notification.
- The status line an agent sets on its own fork is shown after Silo's.

## Open

- When a fork stops progressing, does Silo stop its agent, or only flag it and tell you?
  - Suggested: Flag it and tell you; stopping is a setting, since a slow fork can look stuck.
- To find out: What Claude Code and Codex write, and where, when they stop on a limit, a refusal or an error.
