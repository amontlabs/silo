# Does the engine hold one use of a mechanism?

## The choice

An agent's part, 27 November 2026. polarzero's product principle, said on 27 November, is a strong
core with few opinions, and opinionated extensions on top, some of them shipped with Silo. The
sentence of 20 November says the first half. It does not say where a use goes, and without that
each feature is argued again.

One case each way. Reading that a model call used 12,000 tokens is the mechanism: only something
outside the computer can see it, and it is true whatever you think of it. Saying that 12,000
tokens cost $0.18, and that $20 a day is enough, knows a price list and takes a position. polarzero
ruled on that case: `spend-and-budgets-ship-as-an-extension`.

The test this entry proposes: the core is what only Silo's position outside the computer can do,
and that holds no view on what is good or bad. It isolates a computer, sees, writes the record,
makes a request wait, keeps a secret out of the computer, goes back to a checkpoint and runs
extensions. A reader of a format, such as a model provider's messages, is part of seeing.

The options:

- The principle, with where a use goes (recommended). It costs an extension interface strong
  enough to carry what moves out. Today it is not: `rule-may-count`,
  `request-content-to-an-extension` and `finding-of-an-extension` say what is missing.
- The principle as written on 20 November. It costs the question "core or extension" for every
  feature.
- No principle. It costs a core that grows by one use at a time.

Recommended: the principle, with where a use goes.

Entries it would move, each with a decision of its own: `held-calls-ship-as-an-extension`,
`first-extensions-that-ship`. `uploads-to-your-places` names GitHub and Google hosts in a decision
of the boundary, and `built-in-classifiers-run-inside` would run some of Silo's own classifiers
inside Silo. Both go against it as written.

## The earlier choice, 22 November 2026

An agent's part, 22 November 2026. You want a classifier that judges tool calls. Silo could have
"judge tool calls" built in. Or it could run any classifier you define, on any input, and ship the
tool-call one as an example.

The options:

- A principle (recommended). Silo's engine holds the general thing, and each use is something you
  can change or replace. It costs more to design, and every use needs a ready-made example so that
  Silo works before you set anything.
- Build the uses people ask for. The first ones arrive sooner and need no setup. It costs a change
  inside Silo for every new use, and users who cannot adapt what they got.

Recommended: a principle. polarzero asked for classifiers to be "very customizable", and this is the
general form of that. Engineers know it as the separation of mechanism and policy.

## Notes

polarzero said it about what a classifier's program does, and asked that it be "very customizable". Those cases are the decisions `what-defines-a-classifier` and `what-ships-with-silo`.

The general sentence is an agent's, put forward on 20 November 2026. polarzero has not answered it.
