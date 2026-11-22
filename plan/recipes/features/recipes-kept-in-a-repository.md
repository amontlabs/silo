# Recipes kept in a repository

## The choice

An agent's part, 22 November 2026. Your repository silo-site holds a recipe for macOS computers and
one for Linux computers, and the Linux one names registry.npmjs.org. An agent in main adds paste.rs
to it. When the next test computer is made, Silo shows you the new host and lets it through only
with your yes.

The options:

- Build it (recommended). A repository's computers are described in the repository, change in the
  same commit as the code that needs them, and come with every clone. It costs one more yes each
  time what a recipe names outside the computer changes.
- Leave it out. Recipes are kept only in Silo, apart from any repository. It costs keeping them in
  step with the code by hand, and testing your changes on both systems loses the recipes it makes
  its computers from.

Recommended: build it. Development containers already work this way, and a host or a secret the file
names is never switched on without your yes.
