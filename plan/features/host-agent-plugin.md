## Proposed

- Risk: An agent program's own view cannot show or answer approvals, misses work started by background jobs and can be broken by its vendor, so the plugin is never the only surface.
- Docs: Each plugin or skill states strongly and explicitly when to use Silo and how.
- The plugin shows Silo's forks in the agent program's own task view, starting with the agent program most early users run.
- Rule: The plugin's background monitor exits silently where Silo is not in use.

## Open

- Which agent programs get a plugin or skill: Claude Code only, as planned, or also Codex, Hermes, OpenClaw and pi?
  - Suggested: Claude Code first, then only those you use yourself, because agent-lives-inside makes an agent on the Mac a side path.
- To find out: Does a /skill-style entry point work as the standard way an engine introduces itself to an agent on another device?
- To find out: Is MCP apps the format to use when an agent shows a Silo interface, such as a diff summary, inside an agent program?

## Notes

- An MCP server over the engine's calls plus a skill, published through a self-hosted plugin marketplace, as LCU does.
- LCU gains an `--in-silo` option and a README section on running it inside a Silo computer instead of on the desktop.
- The old `silo` MCP tools (status, fork, exec, diff, merge request, drop) were meant to absorb this plugin.
