# Does the engine hold one use of a mechanism?

## For the owner

An agent's part, 22 November 2026. You want a classifier that judges tool calls. Silo could have
"judge tool calls" built in. Or it could run any classifier you define, on any input, and ship the
tool-call one as an example.

The options:

- A principle (recommended). Silo's engine holds the general thing, and each use is something you
  can change or replace. It costs more to design, and every use needs a ready-made example so that
  Silo works before you set anything.
- Build the uses people ask for. The first ones arrive sooner and need no setup. It costs a change
  inside Silo for every new use, and users who cannot adapt what they got.

Recommended: a principle. You asked for classifiers to be "very customizable", and this is the
general form of that. Engineers know it as the separation of mechanism and policy.

The owner said it about what a classifier's program does, and asked that it be "very customizable". Those cases are the decisions `what-defines-a-classifier` and `what-ships-with-silo`.

The general sentence is an agent's, put forward on 20 November 2026. The owner has not answered it.
