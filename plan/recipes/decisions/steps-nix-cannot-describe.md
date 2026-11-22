# What Nix cannot describe

## The choice

An agent's part, 22 November 2026. Your recipe for a macOS computer includes Microsoft Excel, which
Nix does not package, and computer use. Silo puts the system in place, installs Excel at the version
the recipe names and checks it, then sets the approvals computer use needs.

The options:

- A step that fails stops the making, except an approval (recommended). If the Excel download fails,
  no computer is made, and Silo says which step failed and why. An approval Silo cannot set is the
  one exception: the computer is made, and Silo names the approval you give once by hand. It costs a
  computer you wait for again when one download fails. A vendor that serves only its latest version
  also breaks the recipe until the recipe moves on to it.
- A computer made with what worked. Excel is missing, and Silo says so. It costs the promise that a
  recipe makes the same computer again: the next agent finds Excel missing in the middle of its
  work.

Recommended: stop, except an approval. A recipe is worth having because it makes the same computer
every time, and an approval is the one step you can finish in a minute.

## Notes

A proposal, built on the research in `nix-under-recipes`.
