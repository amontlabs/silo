# What does a task's computer get from main

## The choice

An agent's part, 22 November 2026. An agent in main has changed src/login.ts and added
tests/login.test.ts, and committed neither. It asks Silo to test the changes. Silo takes a
checkpoint of main, reads the repository from it, and gives the fresh Linux and macOS computers one
commit of what it found. Nothing is written to main.

The options:

- Everything not ignored (recommended). The commit holds the last commit, every change not yet
  committed and the new files, without the files the repository ignores. It costs one case to watch
  for: a file nobody listed in .gitignore, such as a stray .env, goes into the task's computers too.
- Committed changes only. It costs a commit before every test, and the agent's last change is not
  what gets tested.
- Changes to tracked files only, no new files. It costs the new test file: the run fails for a
  reason that does not exist in main.

Recommended: everything not ignored. It is what "test my changes" means to anyone who runs the tests
by hand, and .gitignore is where people already say what stays out.
