# One Nix store for macOS computers

## The choice

An agent's part, 22 November 2026. You have two recipes, one for main and one for your docs site,
and both include Node.js and Python. Three computers are made from main's recipe.

Sharing one store of packages from your Mac into computers is ruled out by `device-not-shared`,
which you took: nothing from your device is shared into a computer. What is left for you is the disk
this costs.

The options:

- Packages in each computer, one recipe's computers sharing blocks (recommended). The three
  computers of main's recipe start as copies of one disk and store Node.js and Python once. The docs
  recipe stores them a second time. It costs the disk of what two recipes hold in common, which is
  still to measure.
- One store on your Mac, shared read-only into every computer. Node.js and Python are stored once
  for all recipes. It costs a change to `device-not-shared`, and the share is one more thing an
  agent inside can attack to reach your Mac.

Recommended: packages in each computer. Nothing on your Mac is within reach from inside a computer,
and the disk it costs is only what two recipes have in common.

## Notes

To find out. Linux computers can share one store, read-only (research of 18 November 2026, in
`nix-under-recipes`).

Also to find out: what the first build of a recipe costs in time and disk.
