# Does each agent program's own account match the boundary

## For the owner

An agent's part, 22 November 2026. You turn on the check against Claude Code's own account for main.
On 19 November, behind an ordinary inspecting proxy standing in for Silo, Claude Code 2.1.327
matched on every request: the same request id, model and token counts. Codex has not been compared
yet.

The options:

- Only where a run matched (recommended). For each program and version, a run of real work through
  Silo compares the program's account with what Silo saw, request by request. The check is offered
  only when every request matched, and the app says which programs have no such check. It costs the
  check for Codex and any other program until it is compared, and for a new version until it is
  compared again.
- For every program that gives an account. The check is offered at once. It costs false alerts: a
  program that leaves some of its own requests out of its account, or counts tokens another way,
  raises a mismatch on every such request, and you learn to ignore the alert that should mean
  something.

Recommended: only where a run matched. A mismatch is meant to be a rare sign that something went
around the agent program, and it means that only for a program whose account agrees with Silo when
nothing is wrong.

## Notes

To find out, for each program.

- In the test of 19 November 2026, Claude Code's own report matched the boundary on every request: the same request id, the same model, the same token counts. The request id joins the two accounts.
- Still to find out: the same for Codex and the other programs.
