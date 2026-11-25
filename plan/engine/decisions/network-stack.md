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

## gvproxy as Silo's stack, measured (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-gvproxy-component/`. Run on Linux, in
network namespaces, with no virtual machine and no Mac.

- #363's `-sealed` patch filters TCP and UDP. gvproxy 0.9.0 added ping forwarding, so a sealed
  computer still pings the Mac's own address and every machine on the local network.
- A patch of about 320 lines adds five options to gvproxy: no API on the computer's gateway, no
  address that leads to the Mac's loopback, a list of destinations that no TCP, UDP or ping may
  reach, lookups sent to a server Silo names or refused, and a hand-off of every TCP connection to
  Silo's boundary, after a PROXY protocol v2 header, so that gvproxy dials nothing. In 21 cases
  from inside a computer, it reaches nothing of the Mac or the local network. Closed, it reaches
  nothing at all and SSH still gets in. With the boundary stopped, it reaches nothing.
- The options cost nothing measurable against stock gvproxy: 1.4 Gbit/s out and 1.1 Gbit/s in at
  MTU 1500, 4.3 and 3.1 at MTU 65520. The hand-off adds about 0.1 ms per round trip. Over the
  datagram socket a Mac uses, traffic into the computer at MTU 1500 was 0.28 to 0.90 Gbit/s, which
  the Mac kit should measure on a Mac.

What I would choose, with #378's card that leads nowhere as one of the paths:

- The first build: #378's card and proxy for a closed main and a main limited to a list, with no
  gvproxy.
- The boundary takes connections both from that proxy and from gvproxy's hand-off, from its first
  version.
- An open main with rules, where every program works unchanged: gvproxy with this patch, run as a
  fork pinned to an upstream commit, with the five options proposed upstream so that the fork ends
  if they merge. Proposing them is public, so it is polarzero's call.
- Silo writes no TCP/IP of its own until a measured need. The costs measured come from gVisor's
  TCP engine and the socket that carries frames, so a program of Silo's own on the same engine
  would be no faster.

The answer above stays as it is: every packet still passes Silo, and nothing goes around it when
Silo's part stops.
