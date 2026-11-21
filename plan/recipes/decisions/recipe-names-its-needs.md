# What a computer needs from outside

The owner asked on 21 November 2026, about the secrets of an extension: is a secret named in the
recipe, and is a recipe only what gets inside the computer? It would be weird for an extension to
name its secrets in one place and an ordinary computer in another.

An agent's notes:

- Today a recipe says only what is inside a computer: a pinned base system, then a layer built on
  it (`recipe-shape`). No entry lets it name a secret or a host. For an ordinary computer the user
  gives secrets and sets rules by hand.
- A recipe "sets up a whole working computer" (`what-a-recipe-sets-up`, taken). A computer that
  cannot work without a key is not whole if its recipe cannot say so.
- Known practice: the files that describe a development container or a set of containers name the
  secrets they need, without holding their values.
- With this, an extension's computer needs nothing special: `extension-secrets` in the part
  `extensions`.
