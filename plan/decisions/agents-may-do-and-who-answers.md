# What may an agent inside a computer do

## The choice

An agent's part, 25 November 2026, from the challenge of #352 (file 04) to `what-an-agent-inside-may-do`,
which polarzero took.

The case: an agent in main wants a host it was not given, and Silo asks you. The `silo` command on
your Mac can answer asks. An agent running on your Mac, or a script it wrote there, runs `silo` and
answers yes. The earlier answer says anything that grants more "asks the user", and the command
cannot tell the user from an agent.

The options:

- The user answers in Silo's app or with Touch ID, never by a command (recommended). 1Password does
  the same with Touch ID per terminal session. It costs an answer from a script, and a prompt you
  see each time. An app with Accessibility on your Mac can still click through, which 1Password
  also says; that is the residual risk.
- Any client may answer, as the earlier answer allows. It costs the case above.

Recommended: the app or Touch ID. It is polarzero's call, because it adds a prompt you see.
