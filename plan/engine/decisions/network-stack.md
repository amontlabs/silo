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

## What a user-space stack costs, measured on Linux (24 November 2026)

An agent's part, 24 November 2026. No virtual machine: a network namespace stood for the computer
and another for the internet, and a small program played the virtual machine monitor, handing the
computer's frames to the stack over a unix socket as libkrun does. Three runs each, the median.
The scripts are in `research/2026-11-24-linux-host/`.

| Path | Computer to internet | Internet to computer | Round trip | New connections per second |
|---|---|---|---|---|
| The kernel, with an nftables allow-list | 14.7 Gbit/s | 14.3 Gbit/s | 36 µs | about 5,000 |
| passt | 1.3 to 3.0 Gbit/s | 13.3 Gbit/s | 102 µs | about 2,800 |
| gvproxy, MTU 1500 | 1.4 Gbit/s | 0.96 Gbit/s | 187 µs | about 1,580 |
| gvproxy, MTU 65520 | 4.1 Gbit/s | 3.0 Gbit/s | 193 µs | about 1,490 |

- The kernel path cannot be Silo's boundary: nftables sees addresses and ports, not a request or a
  secret. It shows what a user-space stack costs against a plain machine: 3 to 15 times the
  throughput and 3 to 5 times the round trip.
- An `npm install` or an image download rarely passes 1 Gbit/s, so it is not slowed. Two computers
  that exchange gigabytes through the boundary are.
- gvproxy at MTU 1500, the usual setting, was under 1 Gbit/s into the computer, and three times
  faster at MTU 65520. Silo's stack should take large frames from the start.
- These are a floor for Silo's stack, which also reads HTTP. Measured on a Linux container, not on
  a Mac.
