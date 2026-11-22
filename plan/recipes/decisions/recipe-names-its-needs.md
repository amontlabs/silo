# What a computer needs from outside

## polarzero's words (21 November 2026)

- About the secrets of an extension: is a secret named in the recipe, and is a recipe only what
  gets inside the computer? It would be weird for an extension to name its secrets in one place
  and an ordinary computer in another.
- Then: what is wanted is the same as for extensions. A recipe can optionally name secrets, hosts
  and anything else that fits. Importing a computer then shows the secrets to fill and the rules
  it will add, just as importing an extension does.

## An agent's notes

- Until now a recipe said only what is inside a computer: a pinned base system, then a layer built
  on it (`recipe-shape`). For an ordinary computer the user gives secrets and sets rules by hand.
- "Anything else that fits" is covered by rules and secrets: everything a computer gets from
  outside passes its boundary.
- A recipe "sets up a whole working computer" (`what-a-recipe-sets-up`, taken). A computer that
  cannot work without a key is not whole if its recipe cannot say so.
- Known practice: the files that describe a development container or a set of containers name the
  secrets they need, without holding their values.
- With this, an extension's computer needs nothing special: `extension-secrets` in the part
  `extensions`. Importing an extension is `import-then-install` there.

## Found by an audit, 22 November 2026

An agent's part. This says a recipe may name the rules its computer comes with, and
`recipe-source-in-repository` says rules are never a file in a repository. A repository holds the
recipe of its test computer and names `registry.npmjs.org`; an agent in main adds `paste.rs`, and
`tasks-test-your-changes` switches the rule on. The audit proposes that the source may name rules
and secrets as proposals, which Silo switches on only with your yes, and that the rules in force are
kept by the engine and never read from the repository.

The question is put to polarzero in `recipe-source-in-repository`, under "The choice".
