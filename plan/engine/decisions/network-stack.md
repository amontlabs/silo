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

## A closed network with no stack

Agent, 25 November 2026, from `research/2026-11-25-techniques-from-two-tools/`. A third option
for the first build, between Apple's NAT and a patched gvproxy: main gets a network card whose
other end, on the Mac, answers ARP for one router address and drops every other frame. Main has
no route and no name server. The only way out is a proxy at `127.0.0.1:3128` in main, carried over
vsock to Silo on the Mac, which checks each `CONNECT host:port` against the rules and looks the
name up itself. A packet that does not go to the proxy goes nowhere, so every packet that leaves
passes Silo, as the answer asks, with no stack to build or patch.

Measured on Linux with a Unix socket in place of vsock: Claude Code worked through the proxy, each
of its connections was logged by host name, and without the proxy settings it reached nothing.
What it gives up: a program that ignores proxy settings, UDP (QUIC, DNS), ICMP and raw TCP to other
ports get nothing, where Silo's stack would carry them under rules. An open-source tool already
runs macOS 27 guests this way; a macOS guest needs the ARP answer, or it stays offline.

Recommended for the first build over Apple's NAT: it is closed or allowlisted from the first day,
and it does not wait on #359's test 07. Silo's full stack stays the answer for later.
