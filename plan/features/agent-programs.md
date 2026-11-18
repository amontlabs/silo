## Proposed

- Silo writes no agent program of its own and bundles none in the app. It runs the vendors' programs.
- Any agent program or script runs in main or in any fork and gets all of Silo, including a program Silo does not offer to install.
- The engine names no agent program and depends on no vendor's command, flags or session lifetime. The list of programs Silo offers to install is data.
- Silo never hides or wraps the agent program's own interface, prompt or live output. The diff is added beside it.
- Silo integrates with an agent program only through its documented surfaces: the command line, MCP and hooks. The one exception is the list of sign-in files Silo keeps out of forks and exports.

## Open

- Does Silo add a prompt to the agent program, or does the agent learn of Silo only from its commands and docs? Without a prompt the program stays unmodified, and the agent forks unasked only if the user's own instructions tell it to.
- Is Claude Code the agent program Silo fits first, so that a Claude Code user changes nothing in how they work? Fitting one first focuses the testing; today Claude Code and Codex are treated alike.
