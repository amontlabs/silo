# Forks

## For the owner

An agent's part, 22 November 2026. You want an agent to try moving your project to Node 24 without
touching main. You fork main. The fork starts with main's disk, its own name and its own screen, and
without your secrets or sign-ins. The agent works there, and main loses nothing.

The options:

- Build it (recommended). Any computer, or any of its checkpoints, can be forked. It costs a cold
  start: the fork boots, and the agent program starts again in it, where you may have to sign in. On
  a Mac, Apple allows two macOS computers at once, so a macOS main can run one macOS fork at a time.
- Leave it out. Risky work happens in main, or in a fresh computer made from a recipe. It costs main
  itself when the try goes wrong, or the time to set up again everything that is not in a recipe.

Recommended: build it. Trying something in a copy of the real computer is what people already do
with snapshots of virtual machines and with Git branches, and here it costs main nothing.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

Trying a tool in a fork, which `when-a-fork-is-made` names, could be a feature of its own.
