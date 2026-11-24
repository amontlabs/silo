# How does a computer find the address of a host

## The choice

An agent's part, 24 November 2026. A fork is limited to `github.com` and `pypi.org`. A package
the agent installs looks up `c2VjcmV0.attacker.example`. No connection to that host is ever
allowed, but the lookup itself travels: the server that answers for `attacker.example` receives
the name, and the name carries the data. The same package can also connect to an address it
already knows, with `github.com` in its handshake.

The options:

- Silo answers every lookup, and a limited computer reaches only what Silo resolved
  (recommended). Silo answers each lookup itself, even one sent to `8.8.8.8`. For a name that is
  not on the list, it answers "no such name" and asks no other server, so the name never leaves
  the computer. A connection is allowed only to an address Silo gave for a listed host, and only
  when the handshake names that host. Silo leaves out of its answers the keys that a program would
  use to hide the name in the handshake, so the check can always be made. It costs a small
  resolver in the boundary. A program that looks up names over HTTPS, at a host that is not on the
  list, cannot look them up in a limited computer. Two hosts that share one address, one listed and
  one not, are told apart only by the handshake. A program that names a listed host in the
  handshake and another one inside the encrypted request reaches that other host, unless Silo
  reads that host's traffic.
- Check only the name in the handshake. Lookups go to any server, and Silo allows a connection
  when its handshake names a listed host. It costs the leak above, and a limited computer can still
  send data out through names. A connection with no name, made straight to an address, has nothing
  to check.
- Lists of addresses instead of names. A rule allows a range of addresses, and names play no part.
  It costs every host behind a content delivery network, whose addresses change and are shared
  with thousands of other sites.

Recommended: Silo answers every lookup. A limited computer should not be able to send data
through a channel the list does not show, and the cost is one small piece of the boundary that
Silo builds anyway.

## Notes

Agents' research of 24 November 2026, from the products' own documentation.

- The leak through names is the most common failure found in 2026. One cloud service for agents
  allowed every connection to `8.8.8.8` as soon as a single domain was allowed. A flaw in a local
  product for agents let data out through lookups, past a list of hosts that covered HTTP only.
  Another product's own documentation says that, with ordinary lookups turned on, "DNS can be used
  to exfiltrate data".
- Claude Code's sandbox fixed a related hole in version 2.1.284. Before it, the proxy "connected to
  whatever address an allowed hostname resolved to", so a name on the list that pointed at a
  private address reached it.
- Fly's Sprites works the way recommended here: "Raw IP connections are blocked unless the IP was
  resolved from an allowed domain".
- The keys that hide the name are those of Encrypted Client Hello, which a program finds in a
  lookup's answer. Without them in Silo's answers, a program sends the name in the clear.
- In an open computer, the lookups are still worth having: the first lookup of a name is the
  earliest sign of a host seen for the first time (`first-seen-host`).
- Cloud metadata and your device stay out of reach whatever a lookup returns
  (`device-out-of-reach`): an answer that points at a private address does not open it.
