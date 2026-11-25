# What carries a computer's network at the boundary

## The choice

An agent's part, 25 November 2026. The drafts disagree here, and the choice of the first build decides it.

The case: polarzero starts main for the first time. Stock gvproxy, the driver #352 proposes, serves an
API on the computer's gateway that opens ports on every interface of the Mac, with no flag to turn it
off: one `curl` from inside made it listen on `0.0.0.0:2299` (#358). It also reaches the Mac's
loopback and a service on its LAN address. #363's 64-line patch, `-sealed`, closes all three, and the
internet still works. gvproxy carried 1.4 Gbit/s at the usual frame size on Linux and 4.1 with large
frames, against 14.7 for the kernel (#366).

What each draft says:

- #352, #348, #364 and #370: the first build runs main on Apple's NAT until the sealed gvproxy passes
  the Mac kit's test 07, shown as open, not checked and not recorded, and in reach of services on
  your Mac that listen on every address. This answer would then read "never Apple's NAT once rules
  exist".
- #373 (option A): the first build records model calls and hosts, which are facts only when every
  packet passes the boundary, so main is on Silo's stack from its first start, as this answer
  already says, and test 07 and #358's fix come first.

The options:

- Silo's stack from main's first start, gvproxy only as the sealed copy (recommended if polarzero
  takes option A). It costs test 07 passing before main starts.
- Apple's NAT in the first build until test 07 passes (with option B, or if test 07 fails). It costs
  a first build whose main reaches your network unchecked, which the first run has to say.

Recommended: Silo's stack from the start, with the sealed gvproxy, since #373 recommends option A. If
polarzero takes option B, the second option and #364's wording apply instead. #358's proposed
property, that nothing inside reaches the stack's own services except name lookups
(`nothing-inside-reaches-the-stack`), goes with it; the hostile test of #363 checks it.

## The earlier choice

The choice as it stood before 25 November 2026. The answer it recommended is now an option of the choice above.

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
