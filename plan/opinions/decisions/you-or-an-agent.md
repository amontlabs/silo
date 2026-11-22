# How does Silo know whether you or an agent did something in a computer

## For the owner

An agent's part, 22 November 2026. In main you open a terminal over SSH and run `npm install`. Then
you start Claude Code in that terminal, and it installs pnpm. At 14:05 a scheduled job inside main
rotates its logs.

The options:

- You, a run, or unattributed (recommended). The npm install is yours, the pnpm install belongs to
  Claude Code's run, and the log rotation is unattributed. It costs some names you may find odd: a
  background service you set up yourself shows as unattributed, an agent program Silo does not know
  shows as you when you start it from your terminal, and the commands an agent app on your Mac runs
  itself count as yours. Each name is an opinion, since an agent inside can make its work look like
  yours. A promise in force, that what runs wrote adds up to what changed, has to count what you did
  too.
- Everything that is not a run is you. The log rotation is yours too. It costs honesty: work nobody
  is shown to have done is put on you.
- Only runs. The npm install and the log rotation are both unattributed. It costs your own work
  being mixed with what nothing explains.

Recommended: you, a run, or unattributed. Silo then names someone only when it has a reason to.
