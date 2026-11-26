# What a computer on a shared host can reach

## The choice

An agent's part, 25 November 2026. Ana runs a development server on the host that listens on
every address, port 8088. An agent in Ben's computer scans the host's addresses.

Measured ([the research](../../../research/2026-11-25-shared-linux-host/README.md), `net.sh`): in every layout tried, including a link per computer,
Ben's computer reached a host service listening on every address. A link per computer keeps
computers from each other, not from the host. #358 and #363 found the same on a Mac, through
gvproxy's route to the host's own addresses.

The options:

- Only the boundary, the rest dropped by the host (recommended). The host's firewall drops what a
  computer's link sends anywhere but its boundary's ports. The engine's own sockets are files in
  each person's folder, not ports. It costs one firewall table for all links, found by an
  interface group the engine gives each link when it makes it: about 3% of throughput, measured
  below.
- What each person allows. It costs a person on the host deciding what another person's
  computers can reach on their behalf.

Recommended: only the boundary. A computer reaches the network through its boundary, as
everywhere else in Silo.

## The firewall, measured

An agent's part, 26 November 2026 ([the research](../../../research/2026-11-25-shared-host-leaks/README.md)). Without a firewall, Ben's
computer reached Ana's server on every host address, at its own link's address and at the host's,
and reached Ana's computer once the host forwarded packets, as a host running Docker does. One
`nft` table of five rules for every link in the computers' interface group refused all three, and
the metadata server still answered. At 200 computers it cost about 3% of throughput to the
boundary (15.0 to 15.3 Gbit/s against 15.6 to 15.8) and nothing measurable on a token request.
Three rules per link instead, 600 in all, cost about 30% for the last computer listed. Not
measured: IPv6, which this kernel lacks; the table is written for both.
