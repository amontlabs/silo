# Putting back one run's work

## The choice

An agent's part, 22 November 2026. Three agents work in main. Agent B, sent to fix the CSS, also
rewrote `config/database.yml` and two scripts in `scripts/`. You put back B's run. The two scripts,
which nobody touched since, go back in one step. Agent A edited `database.yml` afterwards, so Silo
shows it to you instead. In `~/code/site`, Silo says which two commits came from B, and you revert
them with Git.

The options:

- Build it (recommended). One run's work is undone without touching the others'. It costs certainty:
  which files a run changed is Silo's opinion, and a file it changed through another program, such
  as a server it asked to write, can be missed.
- Leave it out. To undo one agent, you restore main and every agent goes back, or you fork from
  before and carry the others' work over. It costs the others' work, or the time to carry it.

Recommended: build it. In a main shared by several agents, undoing one of them without the others is
the way back that case needs.
