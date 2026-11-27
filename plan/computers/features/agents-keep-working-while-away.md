# Agents that work through the night

## The choice

An agent's part, 24 November 2026. You start Claude Code and Codex in main at 23:00 and close the
terminal. At 07:00 you run `ssh main.keep` and find both still at work, or `silo` tells you the Mac
slept from 01:12 to 06:40 because its lid was closed.

The options:

- Build it with main (recommended). It costs a second SSH entry, tmux in main and a power
  assertion.
- Leave it to you. It costs the first night of everyone who does not know tmux and energy
  settings.

Recommended: build it with main.
