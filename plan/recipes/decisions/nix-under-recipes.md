# Nix under recipes

## The choice

An agent's part, 22 November 2026. Your recipe for a Linux computer names Node 22 and Postgres 17. A
computer made from it in March and another made in June hold the same versions, down to the last
file.

The options:

- Nix underneath, unseen (recommended). You write a short list, and Silo builds it with Nix, as
  devbox and Flox do. It costs disk, since people report tens of gigabytes per machine for what Nix
  stores, and the time of a recipe's first build. Both are still to measure. On macOS Nix covers
  only the command-line tools and settings.
- Install steps, as in a Dockerfile. Each step runs as written. It costs the exact rebuild: `apt
  install postgresql` gives a different version on a different day, so June's computer differs from
  March's, and Silo cannot say exactly what is in it.
- Nix that you write. You see and edit Nix itself. It costs ease, since few people and few models
  write Nix correctly.

Recommended: Nix underneath, unseen. It gives what polarzero asked of recipes, machines rebuilt exactly,
and the products that use Nix for development machines all hide it.

## Notes

polarzero, 18 November 2026: "polarzero sees Nix as what recipes need: security, predictability,
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

## How sure the research is

- Each claim of the researchers carries its source. Some rest only on a search summary.
- The security advisories for Nix in 2026 were read through summaries. The counts quoted from them
  need checking.
- The research found no published figure for the time and disk of a recipe's first build.
- Users' reports put a Nix store at tens of gigabytes per machine. They give no workload to
  compare with.
- The product that showed Nix to its users and left it is Railway. That it left for that reason is
  the researchers' reading, not checked directly.
- The findings are kept outside this repository.
