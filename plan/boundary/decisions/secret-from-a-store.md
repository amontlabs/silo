# A secret from the team's secret store

## The choice

An agent's part, 24 November 2026. A team keeps its GitHub machine user's token and its staging
database password in 1Password. A recipe in the repository names both secrets. Today each member
would copy each value from 1Password into Silo's prompt, and copy it again after each rotation.

How the vendors do it in 2026. 1Password's service accounts give a program the values of the vaults
it may read, and `op run` puts them in a process's environment, so the agent can read them. Doppler
gives agents short-lived credentials. The brokers built for agents keep the value out of the agent:
TRM Labs puts a broker "in the agent's request path"; the agent "carries a placeholder — never the
real secret", and the value comes from HashiCorp Vault. Infisical's Agent Vault and 1Password's
OpenShell work the same way. That is how Silo's placeholders already work (`secret-placeholder`).

The options:

- A reference into the store, resolved by the engine when a request uses it (recommended). The
  secret holds `op://Engineering/github-bot/token` instead of a value. The engine fetches the value
  with the person's own access to the store, keeps it in memory, and swaps it in as for any secret.
  A member without access to that vault does not get it. It costs one integration per store, and on
  a host nobody sits at, a token for the store kept in the Keychain.
- Values copied into Silo. It costs a copy per member and per rotation, and a revoked value that
  still works in Silo until someone notices.
- Values put in the computer with the store's own tool. It costs the rule that a secret's value
  never enters a computer (`secret-values`).

Recommended: a reference resolved by the engine. Which stores come first, and how the engine
signs in to each on a host nobody sits at, are details settled when it is built.
