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

## Five more programs (24 November 2026)

An agent's part, 24 November 2026. The same check, through the spike of the boundary rather than a
fake provider that arms the probe itself. The fake model asked for a shell call to deny, a harmless
shell call, a file write to deny and a harmless write. Linux, fake keys. The research is in
`research/2026-11-24-every-agent-program`.

| Program | Denied shell call, as the model got it | Denied write | Went on |
|---|---|---|---|
| Gemini CLI 0.71.1 | `spawn EPERM` | `EPERM … open '…/DENIED.txt.<uuid>.tmp'` | yes |
| Qwen Code 0.28.4 | `spawn EPERM` | `EPERM … open '…/DENIED.txt.<hex>.tmp'` | yes |
| Cline 3.0.75 | `Command failed: EPERM … posix_spawn '/bin/bash'` | `Editor operation failed: EPERM` | yes |
| Goose 1.60.2 | `Failed to spawn shell command: Operation not permitted` | `Failed to write …: Operation not permitted` | yes |
| OpenCode 1.19.6 | `EPERM … posix_spawn '/usr/bin/bash'` | | no: its own `git` failed and it stopped (`own-processes-in-a-denial`) |

- Nothing of a denied call ran, and each program sent the error once, with no retry. The allowed
  calls after it ran.
- Only Claude Code and OpenCode mark the result as an error (`is_error`). Cline sends
  `"success": false` inside the text, Goose only the text. A precaution that reads results cannot
  rely on the flag.
- Aider uses no tool calls: it asks the model for edits as text and applies them itself. There is
  no call for the boundary to deny, so for Aider a denial of a tool call does not apply.
- The resume of the conversation was not tried for these programs.
