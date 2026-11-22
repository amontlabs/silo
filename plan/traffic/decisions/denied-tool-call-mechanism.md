# How Silo makes the error for a denied tool call exist

## The choice

An agent's part, 22 November 2026. A rule on main denies `git push --force`. Claude Code, on your
Max plan, gets a reply in which the model asks for `git push --force origin main`.

The options:

- A harmless stand-in (recommended). Claude Code gets the call with its command swapped for one that
  only says Silo denied it, and runs that. On Claude Code's next request, Silo puts the model's own
  call back, with the denial as its result. Nothing extra is sent to Anthropic. It costs a stand-in
  for each tool of each program, and a tool without one cannot be denied. Claude Code's window shows
  the stand-in, not the model's call. It is not tested yet.
- Silo answers the model itself, polarzero's idea of 20 November. Silo keeps the reply from Claude Code,
  sends Anthropic the conversation with the call and the denial, and passes on the model's next
  reply. It costs one more model request for each denial, paid on your plan. That request carries
  your Max sign-in though Claude Code did not send it, and Anthropic's terms forbid a developer "to
  route requests through Free, Pro, Max, or Ultra plan credentials on behalf of their users". Claude
  Code's copy of the conversation also lacks the call and the denial, so Silo has to add them to
  every later request.

Recommended: a harmless stand-in. The model sees the same thing as with polarzero's idea, and Silo never
sends a request in your name.

## Notes

Left for when it is built. Silo sits before the agent program, which is what runs the tool.

## polarzero's way (20 November 2026)

polarzero: do exactly what an agent program's automatic mode does, and "simply return the denial
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

## polarzero's answer (21 November 2026)

polarzero holds that returning the denial to the provider as the tool's result works as it does in
an agent program, and sees one real issue: Silo sending a request of its own. It is decided later,
in the Workshop.

An agent's note, not shared by polarzero: the provider keeps no conversation, so the agent
program's later requests carry neither the call nor the denial unless Silo adds them. To test when
this is built.
