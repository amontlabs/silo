# Whether anything done inside a computer can change its precautions

## The choice

An agent's part, 22 November 2026. A precaution pauses main when a classifier says "not safe". An
agent in main, with `sudo`, tries to switch that precaution off: it edits files, ends processes and
runs the `silo` command.

The options:

- Promise it (recommended). None of that changes the precaution, and tests that attack a real
  computer from inside keep it true. It costs those tests, which must pass before Silo says it in
  public. It also means an agent can never adjust a precaution, even to tighten it.
- Do not promise it yet. Precautions are kept outside, and Silo claims nothing more. It costs the
  precaution its use against a hostile agent, which is the agent Silo assumes.

Recommended: promise it. A precaution an agent could switch off would protect only against agents
that mean well.

## Notes

Put to polarzero on 19 November 2026, who did not object. That a precaution is kept outside the
computer is in force (`where-precautions-live`); that nothing done inside can change it is not.
