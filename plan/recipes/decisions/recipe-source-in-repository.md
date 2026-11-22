# A recipe's source in a repository

The lesson behind `recipe-stored-by-silo` and `no-project-folder` was that an agent can edit a
rules file that sits in a repository. This answer leaves rules out of the repository and lets the
description of the machine in.

Whether this is how the owner wants it is open.

## For the owner

An agent's part, 22 November 2026. A repository holds the recipe of its test computer, and the
recipe lists the hosts that computer needs. An agent in main adds a paste site to that list, then
asks for a test computer made from the recipe.

The options:

- Only with your yes (recommended). What the file lists is a proposal. Silo shows it and switches it
  on with your yes. It costs you one more yes each time the list changes.
- No rules in the file. A recipe in a repository describes only the machine. It costs you setting
  its hosts and secrets in the app by hand, and it goes against your decision that a recipe may name
  them.

Recommended: only with your yes. It keeps your decision that a recipe names what it needs, and an
agent still cannot give itself a host.

The audit's finding is in the notes of `recipe-names-its-needs`.
