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

## Notes

An agent's part, 25 November 2026 (`research/2026-11-25-boundary-end-to-end`). Reading any host a
secret goes to needs a certificate per name. The boundary made each on first use, in under 1.5 ms,
and kept it. Python 3.13 refuses a certificate without an authority key identifier, which OpenSSL
adds only when asked. Each certificate starts a day back, so a computer whose clock fell behind
takes it, and Silo's authority must start in the past too: one made at setup was refused by a
computer 2 h behind.

The boundary of #347 and #374 offers only HTTP/1.1. curl and Node fall back. A gRPC client
cannot, so #379's swap inside HTTP/2 headers has nowhere to run until the boundary speaks HTTP/2
on the hosts it reads that serve gRPC.

An agent's part, 25 November 2026 (`research/2026-11-25-boundary-conflicts`). The boundary now
speaks HTTP/2 on hosts Silo reads that are marked for it, with nghttp2 on both sides. A gRPC client
in each computer called a Google API with its placeholder: one call and one stream with trailers.
Both reached the host with the real token. Before the change every call failed in the TLS handshake,
since the boundary offered only HTTP/1.1. Each request's headers go through the same checks and swap
as on HTTP/1.1. A refused gRPC call gets `PERMISSION_DENIED` with Silo's sentence, which the client
prints. Model hosts stay on HTTP/1.1, where #374 counts calls. Not built yet: offering HTTP/2 only
where the host offers it, which needs the boundary to reach the host before it answers the program.
