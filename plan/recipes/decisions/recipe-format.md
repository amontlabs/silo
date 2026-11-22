# The format of a recipe

## For the owner

An agent's part, 22 November 2026. Your recipe for main is five short lines: the system, three
pieces, two packages, the hosts it reaches and the secret GITHUB_TOKEN. Silo started it from your
project's devcontainer and listed the one setting it could not carry over.

The options:

- A short list of Silo's own (recommended). You read a recipe in a minute, and a model writes one
  correctly. Silo starts one from your project's Dockerfile or devcontainer, and lists what it could
  not carry over. It costs a small new format to learn.
- An existing format as it is, such as a devcontainer or a Dockerfile. People and models know it
  already. It costs the exact rebuild, since these files run install scripts, and the same file
  gives different versions on different days. None of them describes a macOS computer.

Recommended: a short list of Silo's own. It keeps what an existing format gives you, a start from
your project's files, without the scripts that make two builds differ.

## Notes

Marked Notes in the design record. See "Against it" in `nix-under-recipes`.
