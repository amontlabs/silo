# What is only here

## For the owner

An agent's part, 22 November 2026. Main holds 60 GB. Silo lists what is only here: an untracked
`.env` in `~/code/site`, a commit in `~/code/api` that was never pushed, a Postgres database, an
edited `CLAUDE.md`, and `jq`, which an agent installed by hand and which is not in main's recipe,
with an offer to add it. The 20 GB of `node_modules` and build output are not on the list, since a
rebuild brings them back.

The options:

- Build it (recommended). One list of what nothing would bring back, with anything Silo cannot place
  on it. It costs a longer list when Silo is unsure, and the list is Silo's opinion: it reads the
  state of Git inside the computer, which an agent can change.
- Leave it out. You find what is worth saving by reading the disk yourself. It costs the list that
  you see before a drop and before a fork becomes main.

Recommended: build it. It reduces a whole machine to the few things worth saving, and when unsure it
errs towards showing too much.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

It says, as `how-a-path-comes-back` does, that a path Silo cannot place counts as "only here". This feature is the list Silo shows; the other is the classifier behind it.
