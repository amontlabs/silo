# How the code reaches a Linux computer beside a macOS main

## The choice

An agent's part, 22 November 2026. An agent in main has changed 12 files of `~/work/app` without
committing them, and wants to run the tests on Linux.

The options:

- A Linux computer made from the recipe, with the code as a commit (recommended). Silo makes the
  Linux computer from the repository's recipe and hands it one commit of main's current work, the 12
  changed files included. It costs a recipe that works on Linux, and anything outside the
  repository, such as a local database, does not come along.
- Main's folder shared into the Linux computer. The Linux computer sees main's files as they are. It
  costs a path between two computers that a hostile agent in either can use to plant files the other
  runs. It also breaks the build, since `node_modules` and the other build output in main were made
  for macOS.

Recommended: made from the recipe, with a commit. It is how CI already gets code onto a machine of
another system, and it keeps the two computers apart.

## Notes

The code reaching a Linux computer as a commit is described with tasks, in `recipes`.

What it costs: recipes and tasks come early, not late, since a Linux computer beside a macOS main
can only be made from a recipe.
