# Rules a team shares

## The choice

An agent's part, 24 November 2026. Eight developers share the rules for their agents: the
registries allowed, the production hosts denied, merges held for a yes. Today each copies them by
hand into their own tools.

The options:

- Build it (recommended). The team keeps its rule sets in a repository (`team-rule-set`). A set the
  Mac's management installs cannot be switched off (`rules-a-member-cannot-switch-off`), and the
  member sees everything that is managed (`managed-and-shown`). It costs one yes per member for each
  change of a set that is not managed.
- Leave it out. It costs every team the drift between eight copies.

Recommended: build it, in its later stage, with `teams`.
