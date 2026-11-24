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

## Containers inside a computer do not trust Silo's certificate (24 November 2026)

An agent's part, 24 November 2026. Tried on Linux with Docker 29.9.3 and `node:22-alpine`. A fake
host presented a certificate from a test authority that the machine's own store trusts, as the
computer would trust Silo's.

- From a container, Node refused: `SELF_SIGNED_CERT_IN_CHAIN`. `wget`, which reads the image's own
  store, refused too.
- `docker build` of a `RUN wget …` step failed the build: "certificate verify failed".
- With the authority mounted into the container and `NODE_EXTRA_CA_CERTS` pointing at it, Node
  passed.

A container brings its own list of authorities, so whatever the computer trusts stops at its edge.
`docker build` with `npm install` or `pip install` inside is common work for an agent, and those
registries are hosts Silo reads for the age rule (`registry-traffic-read`). Every such build then
fails, with an error that names a certificate and not Silo.

What I would add to the answer:

- **Fail closed, and say why.** A client that refuses Silo's certificate must not get the host
  unread instead, or any agent gets around a rule by using a client that does not trust Silo. The
  record says that a program in the computer refused Silo's certificate for that host, and the app
  shows it beside the failed command.
- **Make the fix one line.** Silo keeps its authority at a fixed path in every computer and tells
  the agent how to use it in a build: copy it into the image and run `update-ca-certificates`, or
  pass it as `NODE_EXTRA_CA_CERTS`, `PIP_CERT`, `SSL_CERT_FILE`. Whether a container runtime in the
  computer can add it to every container by itself is to find out.
