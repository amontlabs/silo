# A classifier asked about each tool call before it runs

The owner said this. Some agent programs have a mode in which a classifier looks at each action
before it runs. The owner wants the same for any agent program.

In the owner's example:

- "safe": the precaution does nothing, and the tool call passes as the rules already allow;
- "not safe": the precaution denies it;
- "unsure": the precaution holds it for you.

Whether unsure is held, and what happens when the classifier does not answer, are defaults to
settle when this is built, with how they are shown and changed: `precaution-on-unsure`. The
owner also said the wait itself always ends; the wait for a classifier is decided in `boundary`.

The owner's words on what it is: "It is caution and not a promise. A classifier can be fooled,
for example by a bad action cut into innocent steps. The rules stay the promise."

What the model sees of a denied tool call is decided in `traffic`.
