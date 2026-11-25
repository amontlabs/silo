# Limits on a secret

## The choice

An agent's part, 25 November 2026, from the challenge of #355 (page 2).

The case: an agent has a GitHub token limited to reading, and it asks GitHub's GraphQL API which
issues are open. GraphQL sends every request as a POST, reads included. The earlier text says a
read-only secret "is sent only with GET requests", which is narrower than `secret-limits`, the
decision it rests on: there, "only read" is GET, HEAD and the requests a rule lists as reads, such
as a Git fetch sent as a POST.

The options:

- "Sent only with requests that read", as `secret-limits` defines them, and limited to named places
  such as repositories (recommended). #355 classified every GitHub REST operation and GraphQL
  mutation, and #353 and #351 agree that a read is decided by the operation, not the method. It
  costs a description of each host's API, which ships with Silo as data.
- Only GET, as the earlier text says. It costs every GraphQL read and every Git fetch.

Recommended: requests that read, and named places. It matches the decision the feature needs, and
the named places stop the case of #350's worm, which pushed its loot to a repository of its own.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

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
