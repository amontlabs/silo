# How a team gives each member the same rules

## The choice

An agent's part, 24 November 2026. A team of eight decides that no agent may reach the production
database host, and that `crates.io` and `registry.npmjs.org` are allowed everywhere. Each of the
eight runs Silo on their own Mac. Today each would have to set those rules by hand, and they drift.

How teams share agent settings today: they commit them to a repository. Claude Code's
`.claude/settings.json` holds a team's allow and deny rules beside the code, and a personal file
that Git ignores holds each person's own additions. Silo already has the pieces for the same thing:
rules are data (`rules-are-data`), a named set is switched on in one step (`rule-sets`), and a
recipe's rules read from a repository are proposals until your yes (`recipe-source-in-repository`).

The options:

- A rule set kept in a repository, switched on with each member's yes (recommended). The set is a
  text file the team reviews like code. Each Silo shows what it would grant before it is on, and
  shows each change again. It costs one yes per member per change, and a member can decline.
- A rule set Silo fetches and applies by itself. Changes reach everyone at once. It costs the rule
  that nothing granting happens without a fact or you (`grant-needs-a-fact-or-you`): a change in a
  repository could allow a host on every Mac with nobody's yes.
- A team server that pushes rules. It costs a server Silo does not run, and it sends the team's
  rules through a place that is not the team's own.

Recommended: a rule set kept in a repository, switched on with each member's yes. A team that
wants rules a member cannot decline has the Mac's management for that
(`rules-a-member-cannot-switch-off`).

The evidence is in [`research/2026-11-24-small-teams`](../../../research/2026-11-24-small-teams/README.md).
