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

## vmnet networks on macOS 27 (24 November 2026)

An agent's part, 24 November 2026. Read, not tried. Since macOS 26 a `vmnet` network object can be
made by an app, and on macOS 27 attached to a virtual machine with
`VZVmnetNetworkDeviceAttachment` ([session 224](https://developer.apple.com/videos/play/wwdc2026/224/)).
The app can "configure the DHCP settings for the network, or add rules to forward TCP or UDP host
ports to specific virtual machines", and pass the network to another process over XPC. So each
computer can have a network of its own, and a port of main can reach your Mac, without Apple's
shared NAT. It is still Apple's network, and the session shows no way for Silo to read or refuse a
packet on it, so it does not replace the stack this answer is about for the boundary. It could
carry a computer whose network is open and unread, or Silo's own way in to a computer.
