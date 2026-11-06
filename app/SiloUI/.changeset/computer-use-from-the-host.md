---
"silo-ui": minor
---

An agent can drive a fork's desktop from the host: `silo mcp` has screenshot, click, type, key, scroll, move, drag and wait tools for a computer, `silo fork --gui` (or the `fork_gui` tool) forks, starts and waits until the desktop and computer-use driver are ready, and `--computer-use auto` sets a fork's approval for the agents inside it to no prompts (forks only; `main` keeps its mode). The host tools drive forks and refuse `main`. Computer-use files no longer show up as changes in a fork's review, and `silo status` shows a computer's computer-use approval.
