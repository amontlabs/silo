# The secrets of an extension

The owner, on 21 November 2026:

- An extension should be able to say which secrets the user has to give, and it then just uses
  Silo's mechanism for secrets.
- Naming them in the manifest would be weird if an ordinary computer names its secrets somewhere
  else: "I think that's more of a recipe thing."

So the manifest names no secret. Whether a recipe names them is `recipe-names-its-needs` in the
part `recipes`.

How a computer uses a secret is in the part `boundary`: `secret-placeholder`, `secret-entry`,
`secret-values`.
