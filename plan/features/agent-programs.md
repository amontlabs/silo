## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes becomes a line of `plan/decisions.json`.

- Silo writes no agent program of its own and bundles none in the app; it runs the vendors' programs.
  - Recommendation: yes. The vendors build them faster than Silo could, and the loop is the part only Silo has.
- Silo adds no prompt to the agent program and replaces none of its tools; the agent learns of Silo only from its commands and docs.
  - Recommendation: talk it through. It keeps the program unmodified, but an agent then forks unasked only if the user's own instructions tell it to.
- Any agent program or script can run in main or in any fork and gets all of Silo, including programs agent-install does not offer.
  - Recommendation: yes. A computer is a whole machine, so nothing stops it; the line keeps any feature from assuming one program.
- The engine names no agent program and depends on no vendor's command, flags or session lifetime; agent-install's list of programs is data.
  - Recommendation: yes. Vendors change their flags often, and keeping names in data keeps the engine stable.
- Claude Code is the agent program Silo fits first, so a Claude Code user can use Silo without changing how they work.
  - Recommendation: talk it through. agent-install treats Claude Code and Codex alike; fitting one first focuses the testing.
- Silo never hides or wraps the agent program's own interface, prompt or live output; the diff is added beside it.
  - Recommendation: yes. While Silo is young, the agent's raw output is how the user learns to trust it.
- Silo integrates with an agent program only through its documented surfaces (the command line, MCP and hooks), never a private interface or file format.
  - Recommendation: yes. It survives vendor updates; logins-stay-put's list of login files is the one exception to name.
