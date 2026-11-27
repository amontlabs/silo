# Secrets from your password manager

## The choice

An agent's part, 24 November 2026. A team's credentials live in 1Password. A recipe names
`GITHUB_TOKEN`, and each member's Silo should find its value there, with that member's own access.

The options:

- Build it (recommended), as `secret-from-a-store` says. It costs an integration per store.
- Leave it out. Values are typed into Silo's prompt. It costs a copy per member and per rotation.

Recommended: build it, starting with 1Password, which developers on a Mac already use.
