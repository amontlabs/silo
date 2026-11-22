# Do the small classifiers that ship with Silo run inside it, or each in a computer of its own

## The choice

An agent's part, 22 November 2026. Silo ships a handful of small checks, such as the one that says
which run wrote a file. The plan calls them built-in classifiers. A Mac holds only a few running
computers, and you decided that what ships follows the same rule as what people add, so each would
take a computer of its own.

The options:

- Simple checks run inside (recommended). A built-in classifier that is only a pattern or a small
  function of Silo's own runs inside Silo. Anything else that ships gets its own computer, like any
  extension. It costs part of your decision, and one difference between what Silo ships and what
  people add.
- Each in its own computer. Your decision holds with no exception. It costs one of the few computers
  your Mac can run for each built-in classifier.

Recommended: simple checks run inside. Your rule keeps other people's code out of Silo, and these
are Silo's own code.

This would replace `shipped-extensions-use-the-manifest`. `what-makes-a-classifier` is reworded to
fit it, and alone sets the word "classifier". The audit's finding is in the notes of that entry.
