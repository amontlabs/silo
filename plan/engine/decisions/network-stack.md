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

An agent's part, 26 November 2026 (research `2026-11-25-gvproxy-upstream`). #380's patch to gvproxy
is now ten patches against upstream: nine written to be sent upstream as separate pull requests and
one, the `-sealed` and `-closed` shorthands, kept only by Silo. If the nine merge, Silo runs
upstream gvproxy with its own flags and keeps no fork. Three of the nine fix bugs that the stack
has today, each shown failing first: `-ec2-metadata-access` does nothing, so #395's metadata path
needs the fix; ping reaches 169.254.0.0/16, which TCP and UDP refuse; and a connection to a port of
the gateway where gvproxy serves nothing goes to the same address on the host's network. #380's
suite, extended, passes every case with the series in all seven settings Silo would use. Sending
anything upstream is polarzero's call.
