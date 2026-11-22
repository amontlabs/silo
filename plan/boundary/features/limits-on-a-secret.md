# Limits on a secret

## The choice

An agent's part, 22 November 2026. You give main a GitHub token limited to reading, until 18:00. The
agent clones and reads issues. A push with that token is refused, and after 18:00 the token is no
longer sent at all.

The options:

- Build it (recommended). A secret can be read-only and can have an end time, and you take it back
  whenever you want. It costs accuracy: "reading" means GET requests, and some services read with
  another kind of request. GitHub's GraphQL API is one, so a read-only token cannot use it. Some GET
  requests also change things.
- Leave it out. A secret is given whole or not at all. It costs you a visit to each service to make
  a narrower token there, where the service offers one.

Recommended: build it. The end time and taking back are exact and useful on every service. The
read-only limit should say in its worst case what it does not cover.
