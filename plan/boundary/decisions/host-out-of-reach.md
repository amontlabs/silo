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
  each person's folder, not ports. It costs a firewall rule per link, set by the engine.
- What each person allows. It costs a person on the host deciding what another person's
  computers can reach on their behalf.

Recommended: only the boundary. A computer reaches the network through its boundary, as
everywhere else in Silo.

Not measured: the host with the firewall rule in place.
