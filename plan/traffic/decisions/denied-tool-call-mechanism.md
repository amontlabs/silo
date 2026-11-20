# How Silo makes the error for a denied tool call exist

Left for when it is built. Silo sits before the agent program, which is what runs the tool.

## The owner's way (20 November 2026)

The owner: do exactly what an agent program's automatic mode does, and "simply return the denial
as the tool output", back to the provider.

Read as a mechanism: Silo does not pass the reply to the agent program. It sends the provider the
conversation with the model's call and a result that says denied, as the agent program would, and
passes the model's next reply to the agent program.

An agent's notes on it:

- For that step the model sees exactly what `denied-tool-call` asks: its own call, unchanged, with
  an error.
- The agent program never receives the denied call, so it cannot run it.
- The agent program keeps the conversation, and its copy holds neither the call nor the denial.
  From its next request on, the model sees only what it wrote afterwards, unless Silo adds the two
  entries back to each later request.
- Silo sends a request the agent program did not send, with the program's sign-in in it. To read
  against the terms, with `terms-on-changing-a-reply` and `anthropic-reads-intermediate`, and
  against "It does not imitate the agent program" of `forwarded-byte-for-byte`.
- Not tested.

## The owner's answer (21 November 2026)

The owner holds that returning the denial to the provider as the tool's result works as it does in
an agent program, and sees one real issue: Silo sending a request of its own. It is decided later,
in the Workshop.

An agent's note, not shared by the owner: the provider keeps no conversation, so the agent
program's later requests carry neither the call nor the denial unless Silo adds them. To test when
this is built.
