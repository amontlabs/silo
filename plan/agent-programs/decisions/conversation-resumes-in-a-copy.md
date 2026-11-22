# Does each agent program take a conversation up again in a copied computer

## The choice

An agent's part, 22 November 2026. You fork main at 14:02 while Claude Code is 40 turns into work on
the login page. In the fork you run `claude --resume`, pick that conversation and go on from turn
40, while the same conversation goes on in main.

The options:

- Checked for each version (recommended). For each version of Claude Code and Codex that Silo's
  recipes pin, a live check takes a conversation up again in a fork, after a restore, and in a fork
  and its parent at once. A version that passed shows that its conversations can go on in a fork.
  One that failed, or was not checked, shows its conversations as staying in the computer where they
  started. It costs a check with a real account and real model calls for each new version, and a new
  version waits for its check before Silo says it works.
- Assumed to work. It costs surprises you find in the middle of work: `--resume` in a fork that
  shows nothing, or two copies of a conversation that change each other's turns.

Recommended: checked for each version. Going on with a conversation in a fork is a promise that only
a check on that version can back.
