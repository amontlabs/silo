# A small computer for an extension

## The choice

An agent's part, 27 November 2026. polarzero wants extensions to be "super easy" to write. A
transaction guard is about forty lines (`transaction-guard`). Under `extension-ships-a-computer`
it runs in a virtual machine of its own. A Mac holds a few running computers, not fifty
(`when-an-extension-runs`), and an idle Debian 12 computer held about 380 MiB in the research
cited by `team-host-size`. Five extensions would then weigh more than main.

The options:

- A small recipe with a number Silo commits to (recommended). `extension-ships-a-computer` stays
  as taken: someone else's code never runs outside a boundary. It costs a recipe to make and to
  keep small, and its size is not measured yet.
- One computer shared by the extensions that ship with Silo. It costs the way the engine knows
  which extension a call comes from, which is by its computer (`extension-registration`).
- Small code runs inside Silo (`built-in-classifiers-run-inside`). It costs the rule that what
  ships uses the same manifest as what people add (`shipped-extensions-use-the-manifest`).

Recommended: a small recipe with a number Silo commits to.
