# Is the check against an agent program's own account optional

## For the owner

An agent's part, 22 November 2026. You have main and three forks, all running Claude Code. Comparing
Claude Code's own account with what Silo saw costs some extra work and disk in each computer, and
alerts you on a mismatch.

The options:

- Off until you turn it on (recommended). You turn the check on for a computer where you want it. It
  costs the check in every computer where you did not think of it.
- On unless you turn it off. Every computer is checked from the start. It costs the extra work and
  disk in every computer. It also costs false alerts from programs whose account is not yet known to
  match, and alerts that are often wrong stop being read.

Recommended: off until you turn it on. Silo already sees every model call itself, and the check is
for the computer where you have a reason to doubt an agent.

## Notes

The owner said this, on 19 November 2026.

Not settled, in the owner's words: "Whether it is on by default, and how it is built, are not settled."
