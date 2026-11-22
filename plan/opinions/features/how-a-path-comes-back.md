# How each path would come back

## For the owner

An agent's part, 22 November 2026. Before you drop the fork try-bun, Silo sorts what is on its disk.
/work/app/node_modules, 1.4 GB, comes back by a rebuild from package-lock.json. /work/app/src comes
back by a pull, since its commits are on GitHub. /work/app/.env and ~/notes.md are only here.

The options:

- Build it (recommended). Every path gets one of three answers, and a path Silo cannot place counts
  as only here. It costs a longer list than needed, until you answer Silo's questions about the
  paths it cannot place. Each answer is an opinion, and for a Git checkout it rests partly on what
  is inside the fork, which an agent can change.
- Leave it out. It costs the list of what is only here, which a drop and a fork that becomes main
  show you first, and the view of a computer's disk. You check the whole disk yourself before a
  drop.

Recommended: build it. It is what lets you drop a fork knowing what you lose.
