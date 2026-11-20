# What the model sees when Silo denies one of its tool calls

- The owner's model is an agent program's automatic mode, where an error comes back from the tool call, saying it was refused, maybe with a reason.
- This rewords a line the owner agreed, the answer of `denied-call-in-the-reply`: "Silo changes the model's reply at the boundary, and records the original and the replacement". That answer stays in force until this one is taken.
- It also rules out turning the call into plain text.

## What differs from an automatic mode

Who runs the tool. An automatic mode decides inside the agent program, which runs the tools and can answer a call with an error. Silo decides before the agent program: the tool has not run, and nobody has produced a result for that call. The target is the same.

## How the error is made to exist

Left for when it is built: `denied-tool-call-mechanism`.

## The owner's reading (20 November 2026)

The owner does not see what differs from an automatic mode: in both, the model sends the tool
call and receives a refusal. The only difference the owner sees is where the classifier is: on the
provider's servers for Claude Code, at the boundary for Silo.

## What an automatic mode does, as read on 20 November 2026

An agent read Anthropic's pages on Claude Code's automatic mode and on the Messages API. The
quotes of the first two lines came through a summarising fetch.

- A blocked action does not run, and "that denial comes back as a tool result".
- The agent program asks the classifier, before it runs the tool.
- The API keeps no conversation: the program sends the whole conversation with each request, and
  writes one result for each tool call of the reply, "even if the tool failed".

Not documented: who writes the result of a blocked call. It follows from the third line that the
agent program does.

## The owner's answer (21 November 2026)

Accept that Silo can do this. How is decided later, in the Workshop, and no more time goes to it
now. The scene stays in film 4, as a good example.

The one open point the owner calls real is Silo sending a request of its own: it is in
`denied-tool-call-mechanism`.

This decision takes the place of `denied-call-in-the-reply`, whose answer named a mechanism: "the
tool call is replaced by a refusal". It sets again the property that decision added, unchanged.

Later that day the owner was not sure this can be approved yet, since how it is done is part of a
feature decided later.

The owner agreed on 21 November 2026, and it is taken.
