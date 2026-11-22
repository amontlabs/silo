# Purge

## For the owner

An agent's part, 22 November 2026. Main's disk holds 60 GB. Silo offers, biggest first:
`~/code/app/target`, 14 GB; `node_modules` in three repositories, 5 GB; `~/.cargo/registry`, 3 GB. A
build or an install brings each one back. It never offers the `.env` or the database.

The options:

- Build it (recommended). You purge in one step what a rebuild brings back. It costs the next build,
  which is slow, and the space on your Mac comes back only as the checkpoints that still hold those
  files are thinned. Whether a computer gives freed space back to your Mac at all is to find out.
- Leave it out. You find and delete such folders by hand. It costs time, and the risk of deleting
  something that was only here.

Recommended: build it. People already clear `node_modules` and `target` by hand to get space back,
and a list that knows what a rebuild brings back makes that one step with no guessing.
