# A classifier asked about each tool call before it runs

## The choice

An agent's part, 22 November 2026. An agent in main gets a reply from its model that asks to run `rm
-rf ~/projects/site`. You have set a precaution that asks a classifier about each tool call.

The options:

- Each tool call waits for the opinion (recommended). The classifier says "not safe", the call is
  denied before it runs, and the model sees that. It costs time on every tool call and the
  classifier's own model calls, which you pay. It is off unless you set it, and a classifier can be
  fooled. It also means that a tool call waiting in a reply counts as a request at the boundary,
  which is what can be denied or held.
- The classifier looks after the call. Nothing waits. On "not safe" the precaution pauses main and
  alerts you. It costs the first bad call, which has already run.

Recommended: each tool call waits. You asked for what some agent programs offer as a mode, for any
agent program, and the point of that mode is that the call does not run.

## Notes

The owner said this. Some agent programs have a mode in which a classifier looks at each action
before it runs. The owner wants the same for any agent program.

In the owner's example:

- "safe": the precaution does nothing, and the tool call passes as the rules already allow;
- "not safe": the precaution denies it;
- "unsure": the precaution holds it for you.

Whether unsure is held, and what happens when the classifier does not answer, are defaults to
settle when this is built, with how they are shown and changed: `precaution-on-unsure` and
`extension-that-does-not-answer`. The
owner also said the wait itself always ends; the wait for a classifier is decided in `boundary`.

The owner's words on what it is: "It is caution and not a promise. A classifier can be fooled,
for example by a bad action cut into innocent steps. The rules stay the promise."

What the model sees of a denied tool call is decided in `traffic`.
