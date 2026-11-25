# What carries a computer's network at the boundary

## The choice

An agent's part, 22 November 2026. An agent in a macOS computer runs `npm install`, and the
computer's rules allow only registry.npmjs.org and github.com. Apple's virtualization comes with a
network of its own, which lets a computer out with no rule.

The options:

- Silo's own network (recommended). Every packet of the computer passes through Silo, on macOS
  computers as on Linux ones, so rules, holds and secrets kept outside all work there. If that part
  of Silo stops, the computer has no network until it is back, and nothing goes around it. It costs
  speed on network-heavy work against a plain machine. That cost is not measured yet, and it is
  measured before anything relies on it.
- Apple's network for macOS computers. Nothing of Silo sits between the computer and the internet.
  It costs every limit at the edge of a macOS computer: no rule, no hold, no secret kept outside,
  and no record of what was sent.

Recommended: Silo's own network. The limits on what leaves a computer are the reason it has a
boundary, and with Apple's network a macOS computer would have none.

## Notes

The record's table of what can be seen per system gives "Silo's network stack" for a Linux
computer, and for a macOS computer "The same. Never Apple's NAT." The first has no mark, the
second is marked Notes.

The same table puts model calls, secrets and budgets at the boundary on both systems. Those lines
have their homes in the parts `boundary` and `traffic`.

An agent's part, 25 November 2026 (`research/2026-11-25-boundary-end-to-end`). One Zig boundary
took both ways in on Linux: #378's `CONNECT` and #380's hand-off. A request on a kept-alive
connection took 0.22 ms with no boundary, 0.53 ms by `CONNECT` and 0.71 ms by the hand-off. Three
faults came up between the drafts.

- #378's relay sets no `TCP_NODELAY`. A reply the boundary writes in two pieces then waits for a
  delayed acknowledgement: 44 ms on every kept-alive request and on the first byte of every stream.
  One line fixes it. The relay on the Mac needs the same.
- #380 runs the hand-off with `-outbound-deny private,host`, which drops `169.254.169.254` before
  the boundary sees it, so #379's metadata server is unreachable. gvproxy's `-ec2-metadata-access`
  does not help: at upstream `197eea05` it never reaches the stack's configuration. With the deny
  list cut to `host` and a one-line fix of the flag, the computer got its Google and Azure tokens.
- #374 tells computers apart by the address a connection comes from. Neither way in gives one: vsock
  has none, and every computer behind its own gvproxy is `192.168.127.2`. The boundary gave each
  computer its own socket on each way in, and its own port for lookups.

An agent's part, 25 November 2026 (`research/2026-11-25-boundary-conflicts`). Each of the three
faults above has a fix, and a test that fails without it.

- #378's relay inside the computer sets `TCP_NODELAY` on the program's connection. A boundary that
  writes a reply in two pieces then answers a kept-alive request in 1.5 ms instead of 44 ms. The
  relay is in the Linux computer on both systems, so this is one line in one place.
- In hand-off mode, gvproxy hands every TCP connection to the boundary before its own checks. Its
  deny list and its metadata check apply only when gvproxy dials out itself, which it never does in
  this mode. With #380's flags unchanged (`-outbound-deny private,host`) and with `-sealed`, the
  computer got its Google and Azure tokens from the boundary. A private address still reached
  nothing: the boundary refused it. Cutting the deny list to `host` was the other fix, but `-sealed`
  puts `private` back in. gvproxy's `-ec2-metadata-access` bug is a separate fix for upstream, with a
  test that fails on `197eea05`.
- The boundary knows a computer by the socket its connection arrives on: its own vsock listener by
  `CONNECT`, its own hand-off socket, its own port for lookups. This needs one gvproxy per computer,
  as #380 runs it. #374, which knows a computer by an address, skipped both its checks for a
  connection from an address no computer has: a key nobody was given reached the provider, and no
  budget applied. Patched, such a connection is refused with "this connection comes from no computer
  Silo knows".
