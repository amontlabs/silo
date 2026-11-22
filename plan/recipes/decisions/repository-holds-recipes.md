# Recipes in a repository

## The choice

An agent's part, 22 November 2026. Your repository silo-site holds two recipes in a file at its
root, one for macOS computers and one for Linux computers. An agent in main asks Silo to test its
changes, and Silo makes both computers from those two recipes.

The options:

- Its own recipes, in the repository or in Silo (recommended). A change that needs a new package
  carries the recipe change in the same commit, and whoever clones the repository gets the recipes
  too. It costs a file that agents in main can edit. What it names outside the computer, such as
  hosts and secrets, still waits for your yes.
- Recipes only in Silo. You attach recipes to a repository in Silo, and the repository holds
  nothing. It costs keeping them in step with the code by hand, and a clone of the repository comes
  without them.

Recommended: its own recipes. Development containers already work this way, and a host or a secret
the file names is never switched on without your yes.

## Notes

polarzero said this on 18 November 2026.

It has to be squared with `recipe-stored-by-silo` and `no-project-folder`. The way put forward is
`recipe-source-in-repository`.
