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

## gvproxy sealed, measured (24 November 2026)

An agent's part. PR #358 found that, with gvproxy's defaults, code inside a computer opens a port on
every interface of the Mac through gvproxy's API, and reaches the Mac's own loopback at
`192.168.127.254`. A 64-line patch, `-sealed`, removes the API from the gateway, maps no address to
the host, and refuses to dial loopback, private, link-local and the host's own addresses. It is in
`research/2026-11-24-red-team-fixes/network/`.

| From inside a computer | Stock | API behind a token | Sealed |
|---|---|---|---|
| The Mac's loopback through `192.168.127.254` | 6 ports | 6 ports | none |
| A port opened on the Mac's every interface | yes | no | no |
| A service on the Mac's LAN address | reached | reached | not reached |
| A host on the internet | reached | reached | reached |
| Silo opens a port from the Mac, on its own socket | works | works | works |

A suggestion for the answer: no address inside a computer reaches an API or a service of the stack,
except name lookups. That is the property `nothing-inside-reaches-the-stack` that #358 proposes, and
the hostile test above checks it.
