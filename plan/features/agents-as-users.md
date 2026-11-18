## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes becomes a line of `plan/decisions.json`.

- Every command, output format and doc of Silo is designed for an agent using it as much as for a person.
  - Recommendation: yes. The agent lives in main and drives Silo from there, so it uses Silo more than anyone.
- An agent's MCP tools are the command line's commands, generated from one definition with the same names, arguments and output.
  - Recommendation: yes. It keeps agents-get-every-tool to one small set of verbs, and calls-and-events already generates the rest.
- Every capability has a name and a command or a button, even one an agent could do from a prompt alone.
  - Recommendation: yes. A capability with no name is never found, by a person or by an agent.
- Everything Silo gives an agent to act on, such as a diff, an error or an approval's answer, is plain text.
  - Recommendation: yes. Agents read text reliably, and the app's views come from the same data.
