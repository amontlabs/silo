# What happens to the model's reply when Silo denies a tool call

Agreed in the discussions of 18 and 19 November 2026.

- `denied-tool-call` puts forward a new wording of this answer: the model sees its own call, unchanged, with an error that says it was denied. This answer stays in force until that one is taken.
- How each agent program takes a changed reply: `changed-reply-per-program`, to find out.
- How Silo makes the error exist from where it sits: `denied-tool-call-mechanism`, open.
