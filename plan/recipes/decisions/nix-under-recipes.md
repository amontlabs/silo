# Nix under recipes

The owner, 18 November 2026: "The owner sees Nix as what recipes need: security, predictability,
and machines that can be rebuilt exactly." The answer is a proposal built on the research below.

## What the research of 18 November 2026 found

- Nix pins every package of a machine to an exact version and checks signatures. It does not show
  that anyone reviewed what was pinned (`recipe-audit`).
- The products built on Nix for development machines hide it behind a short file or a command
  (Replit, Google's Firebase Studio, devbox, Flox). One that showed it to its users left it, for
  that reason among others.
- On Linux, Nix can describe the whole machine. On macOS it covers one layer: command-line tools
  and settings. The base system comes from Apple's installer, apps come through Homebrew, and
  privacy permissions are outside it.
- A machine managed wholly by Nix does not run a program downloaded from the internet without a
  fix. Agents download such programs all the time.
- Nix does not see what an agent installs by other means, and nothing was found that detects it.
  The "only here" list is what covers this.
- Agent programs are packaged for Nix within hours of their release, outside the main package
  set. A module exists that places Claude Code's settings, instructions, skills and servers. None
  was found for the other agent programs.
- The Nix store is heavy. Linux computers can share one store, read-only (`nix-store-shared`).

## Against it

`recipe-format` says a recipe is written in an existing format that people know. This answer says
a user picks pieces and writes a short list. The two are to be squared.
