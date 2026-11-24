# What is in main when the first run ends

## The choice

An agent's part, 24 November 2026. The first run ends and you run `ssh main`. You type
`git clone`. On a fresh macOS, `git` is a stub that opens a dialog to install the Command Line
Tools, and nobody sees that dialog over SSH. There is no Homebrew and no agent program either.

The options:

- The first run installs what agents need, after showing the list (recommended). Command Line
  Tools, Homebrew, tmux, and the agent programs you pick, each by its vendor's own installer. It
  costs time in the first run, and a first build that installs Claude Code without a pinned version
  or checksum, which `claude-code-fetched-by-recipe` asks for once recipes exist.
- A blank main, as `blank-computer` says. You install everything yourself over SSH. It costs your
  first half hour, and the dialog that `git` opens is invisible.
- Recipes first. Main is made from a recipe with pinned versions. It costs the first build:
  recipes need Nix and about forty open decisions.

Recommended: the first run installs what agents need, after showing the list. It is the shortest
way to an agent at work, and the list becomes the first recipe later.

## Notes

Agent, 24 November 2026. From draft #354 (`details/first-run.md`), which times the first run at
about 31 minutes at 300 Mbit/s, most of it the download of macOS. Whether Claude Code stays signed
in over SSH is the second Mac test of draft #352.
