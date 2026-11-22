# Denied tool calls

## For the owner

An agent's part, 22 November 2026. A rule on main denies `git push --force`. Claude Code's model
asks for `git push --force origin main`. It gets back its own call with an error that says Silo
denied it, and goes on with something else.

The options:

- Build it (recommended). A few commands can be denied before they run, by a rule the agent cannot
  remove. It costs a rule that matches only the text: `git push -f` gets through, and the rule says
  so. How the error is made and what the vendors' terms say are still open, and it works only for
  the programs Silo has tested.
- Leave it out. A command that acts only inside the computer cannot be denied before it runs. It
  costs a restore to undo it afterwards, or the agent program's own deny rules, which an agent can
  turn off.

Recommended: build it. A handful of commands are worth denying before they run, and a model that is
told its call was denied goes on with its work.
