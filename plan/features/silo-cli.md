## Proposed, not confirmed by the owner

### For whoever builds it

- Commands
  - Rule: The short form of a command does the safe, common thing, and the unsafe, rare thing needs more typing.
  - Rule: Scriptable verbs (fork, run, test, checkpoint, merge, drop) never prompt, and the few commands meant for a person are marked as an interactive mode.
  - Rule: Flags and output follow git and gh wherever a matching concept exists, as the verbs do under git-words.
  - Rule: Docs, help output, generated agent configs and skills use one command name, and an alias is documented once.
  - Rule: The documented command resolves from any working directory.
  - Property: The command line is one binary that needs no runtime installed.
  - Rule: The command line stays plain text and holds no full-screen terminal interface.
  - Detail: `silo doctor` checks the setup.
  - Property: Whatever the app did can be reproduced from the command line, so an agent can investigate a report such as a hang without the app.
- Output
  - Rule: A command does one thing, prints little, exits on success, and prints nothing when there is nothing to report.
  - Rule: A command that starts long work exits, and the work is a state to query.
  - Detail: Commands return records, rendered in a compact structured form or in the human format, so nothing downstream parses free text.
  - Detail: Every error carries structured diagnostics and a repair hint.
  - Detail: After each action, the command line suggests the next step.
  - To find out: Does a compact format that models already write, such as YAML, beat a more efficient one such as TOON, given that models asked for TOON wrote YAML?
- Help and docs
  - Detail: `silo docs` prints the agent docs of the installed version to standard output and fetches nothing from the latest release.
  - Detail: `silo docs` has a JSON form, and fails with a named error and a non-zero exit.
  - Rule: The command line explains itself on request, like a manual page, and loads nothing into the agent's context up front.
  - Detail: The first command an agent runs prints a complete worked example and the rules of use.
  - Detail: A short set of worked examples, sized to be read in one go, ships with the command line.
  - Rule: Each help text costs few tokens.
- Tests and evals
  - Test: A model given only the shipped help and examples runs each command, and each is used correctly on the first try; the run repeats when commands change.
  - Rule: Each failed first attempt by an agent is a defect, and its prompt is kept as an eval.
  - Rule: Between two candidate shapes for a command name, an argument or an error text, the one agents get right more often on the same tasks is kept.
  - Test: Run every command without sudo and outside any special group, and each works or says plainly what it needs.

## From the old code

The old code had it built and driven end to end, about 7,900 lines, and its approach can be reused.

Lessons:
- Keep one operations layer that both the CLI and MCP call, returning the same JSON, so the two never drift.
- Exit codes are a contract agents script against; changing one breaks callers silently.
- Never touch the production link from a Dev build; names come from one channel module.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-CLI.md#commands`
- `app/SiloUI/cli/src/cli.rs`
- `app/SiloUI/cli/src/ops.rs`
- `app/SiloUI/cli/src/render.rs`
- `app/SiloUI/cli/tests/commands.rs`
- `docs/SiloUI-CLI.md#install`
- `app/SiloUI/host/src/channel.rs`
- `docs/SiloUI-CLI.md#engine-socket`
- `app/SiloUI/cli/src/engine.rs`
- `app/SiloUI/cli/src/endpoint.rs`

Calls: `silo`.
