# How each agent program takes a tool call that the probe makes fail

## The choice

An agent's part, 23 November 2026. An extension that protects main denies
`rm -rf ~/projects/site`, and the probe makes the command fail inside main. Nobody has yet tried
how Claude Code, or Codex, takes a command made to fail this way.

The options:

- Checked per program and version (recommended). A test denies one call of each tool. It shows
  that the program runs nothing of it, sends the error to the model, and goes on working, also
  after the conversation is resumed. A precaution can deny tool calls in a program only once it
  passes. On a version not checked yet, Silo does not pass on a reply whose tool call is denied,
  and the app says so. It costs a wait before tool calls are denied cleanly in a new program or a
  new version.
- Offered for every program at once. It costs a program that misreads the failure: it may stop, or
  try the same call again and again, and you find out in the middle of work.

Recommended: checked per program and version. A denial that breaks the program it protects costs
more than one that comes a little later.

## Notes

This entry was `changed-reply-per-program`, when a denial changed the reply. It was renamed on 23
November 2026, when the way proposed in `denied-tool-call-mechanism` became the probe.

Reading tool calls is tested (`reading-each-agent-program`). Making a call fail inside the
computer is not.
