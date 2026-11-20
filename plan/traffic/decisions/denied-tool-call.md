# What the model sees when Silo denies one of its tool calls

- The owner's model is an agent program's automatic mode, where an error comes back from the tool call, saying it was refused, maybe with a reason.
- This rewords a line the owner agreed, the answer of `denied-call-in-the-reply`: "Silo changes the model's reply at the boundary, and records the original and the replacement". That answer stays in force until this one is taken.
- It also rules out turning the call into plain text.

## What differs from an automatic mode

Who runs the tool. An automatic mode decides inside the agent program, which runs the tools and can answer a call with an error. Silo decides before the agent program: the tool has not run, and nobody has produced a result for that call. The target is the same.

## How the error is made to exist

Left for when it is built: `denied-tool-call-mechanism`.
