# How each agent program takes a reply that Silo changed

## The choice

An agent's part, 22 November 2026. A rule on main denies `git push --force`. Claude Code's model
asks for it, and Silo changes the reply so that the push does not run and the model sees it was
denied. Nobody has yet tried how Claude Code, or Codex, takes a reply changed this way.

The options:

- Checked per program and version (recommended). A test denies one call of each tool a rule can
  name. It shows that the program runs nothing of it and goes on working, and that the model sees
  the denial, also after the conversation is resumed. You can write such a rule for a program only
  once it passes. When a new version of the program is not checked yet, your rules still deny, and
  the app says the program may not take the denial cleanly. It costs a wait before you can deny tool
  calls in a new program or a new version.
- Offered for every program at once. It costs a program that misreads the changed reply: it may stop
  with an error or try the same call again and again, and you find out in the middle of work.

Recommended: checked per program and version. A denial that breaks the program it protects costs
more than a rule you can write a little later.

## Notes

To find out. Reading tool calls is tested (`reading-each-agent-program`); changing a reply is not.
