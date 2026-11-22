# Is an agent program's own account of its model requests used as a check

## The choice

An agent's part, 22 November 2026. An agent in main finds Claude Code's sign-in in its files and
calls the model 40 times with `curl`, outside Claude Code. Silo sees all 40 calls and counts them in
main's spend. Claude Code's own account of its calls holds none of them.

The options:

- Use it as a check (recommended). Silo compares Claude Code's account with what it saw, and alerts
  you to the 40 calls Claude Code does not account for. It costs a reader for each agent program's
  account, and some extra work and disk. It can also raise false alerts for a program whose account
  leaves some calls out, which is still to find out for each program.
- Keep only what Silo saw. The 40 calls are still counted and shown. It costs the sign that they did
  not come from Claude Code, which is one of the few signs that an agent works around what it knows
  is recorded.

Recommended: use it as a check. A program's account cannot be trusted alone, but its disagreement
with what Silo saw is worth knowing, and whether the check is on is a separate choice.

## Notes

polarzero said this, on 19 November 2026.

- What handles a mismatch is in force: `account-mismatch-precaution`.
- Whether each program's account matches the boundary: `own-account-matches`, with the first test.
- Whether the check is optional: `own-account-check-optional`.
