# A fork becomes main

## The choice

An agent's part, 22 November 2026. A fork has spent the morning moving your project to Node 24, with
new packages installed and a database migrated. It works, and you make it main. An agent in main had
committed a fix in `~/code/api` since, so Silo first shows that commit as only in the old main.

The options:

- Build it (recommended). The fork takes main's place, and the old main is stopped and kept aside.
  It costs the agents still running in main, which stop. How the switch works exactly is a decision
  of its own, still open.
- Leave it out. A fork's work reaches main only through Git and copied files. It costs everything
  else done in the fork, the packages, the settings and the database, which you redo in main by
  hand.

Recommended: build it. A try that worked should become the real computer as it is, not be done
twice.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

It rests on `fork-becomes-main`, which is open. Its text says only what the taken decisions say.

## Conversations and their budgets when a fork becomes main (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-forked-conversations`. Claude Code and Codex in main, after main's disk
became a fork's, took up the fork's copy of a conversation, with the same session ids as main's
own. Read by content, its first request continues main's old conversation at the turn before the
fork, so a budget on it counted main's discarded turns and none of the fork's: /bin/bash.2028 instead of
/bin/bash.1696 for Claude Code. What this feature needs: the fork's conversations become main's, with
their spend and their links, and main's own go with the old main kept aside.
