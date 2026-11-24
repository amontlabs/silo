# How does the boundary read encrypted model traffic

## The choice

An agent's part, 22 November 2026. Claude Code in main talks to api.anthropic.com. An install script
fetches from registry.npmjs.org, which your rule on package age names. An agent clones a repository
from github.com.

The options:

- Only the hosts Silo needs (recommended). Silo reads the connections to api.anthropic.com and
  registry.npmjs.org. The clone from github.com passes encrypted and unread, and the record keeps
  its host, size and timing. Nothing reaches a host Silo reads without being read: a program that
  tries HTTP/3 there is refused and falls back to an ordinary connection. Silo checks each real
  host's certificate itself, so nothing inside main can make it trust a false api.anthropic.com. It
  costs what Silo does not read: the record shows that main reached github.com, not which files it
  fetched. A host Silo reads whose certificate comes from your company's own authority also fails,
  with no way yet to make Silo trust it.
- Every encrypted connection. Silo reads all of main's traffic, as a company proxy set to inspect
  everything does, and the record holds the address of every page an agent opened. It costs every
  program in main depending on Silo's certificate: one that refuses it fails on every host, not on a
  few. It also costs a wait on each connection, more disk for the record, and whatever travels in
  those addresses, such as a token in a link, kept in the record.

Recommended: only the hosts Silo needs. Each host Silo reads is a place where a program can refuse
its certificate and break, so it should be only where reading gives something: spend, tool calls, a
package's age, a secret.

## The Host of each request (24 November 2026)

An agent's part. The strict reader in `research/2026-11-24-red-team-fixes/strict/` refuses a request
whose `Host` is not the name in the handshake, or whose target is an absolute URL. Go's `net/http`
takes the host from an absolute URL over the `Host` header, so a request sent under
`api.anthropic.com` could be served as `attacker.example`. A suggestion for the answer: each request
on a connection Silo ends names the host of the handshake, and is refused otherwise.
