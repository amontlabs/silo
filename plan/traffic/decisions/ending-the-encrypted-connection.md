# How does the boundary read encrypted model traffic

## The choice

An agent's part, 25 November 2026, from the challenge of #355 (page 2), with #363 and #356.

The case: github.com is allowed so that agents can clone public repositories, and no GitHub secret
is given. A worm in the computer pushes what it stole to its own repository, with its own token.
Under the earlier answer that connection "passes encrypted and unread", so no rule can see the
token, let alone refuse it.

The options:

- A rule can ask the boundary to read a host it allows, and each request must name the host of its
  handshake (recommended). With `foreign-credential-refused`, a host read this way refuses a
  credential that is not Silo's. #363 and #356 both found that a request naming another host than
  its handshake has to get no secret; over HTTP/2 the answer is the status 421 (#355). It costs the
  encryption of the hosts such a rule names, which Silo then reads.
- Read only the hosts the earlier answer lists. It costs the worm's case.

Recommended: a rule can ask to read a host. The default does not change: a host is read only when it
is a provider's, a registry a rule names, one that gets a secret, or one a rule names.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

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
