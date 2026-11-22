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
