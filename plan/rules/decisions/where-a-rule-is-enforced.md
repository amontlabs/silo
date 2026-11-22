# Where a rule is enforced

## For the owner

An agent's part, 22 November 2026. Main has no rule that allows paste.rs, and an agent in main tries
to reach it. The connection is refused outside main.

The promise you took says: "A rule is enforced by the engine, outside the computer, and each
decision is recorded with the version of the rule." The engine's entries now propose a small program
for each computer, outside it, that holds that computer's network and applies its rules
(`processes`, `which-process-holds-the-boundary`). A crash in one computer's program then leaves
the other computers running. With that design, main's own program refuses paste.rs, not the engine,
and the promise no longer says what happens.

The options:

- Promise "outside the computer" (recommended). The sentence keeps what you rely on: the rule is
  applied where the agent cannot reach, and each decision is in the record with the version of the
  rule. Which of Silo's programs applies it is left to the agents, and can change without changing
  the promise. It costs nothing you see.
- Keep "by the engine". Every rule is then applied inside the engine. It costs the separate program
  for each computer: a crash of the engine stops the new connections of every computer at once.

Recommended: promise "outside the computer". It is the part you rely on, and it stays true whichever
program applies the rule.

## Notes

`rules-out-of-reach` says "A rule is enforced by the engine, outside the computer" too, and follows
this answer. `guarantees-outside-the-computer` already says "parts of Silo that run outside the
computer".
