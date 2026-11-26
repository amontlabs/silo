# How the boundary knows a computer on a shared host

## The choice

An agent's part, 25 November 2026. Ben's computer asks the address 169.254.169.254 for a Google
Cloud token, as `cloud-credentials` proposes (#379 measured it). The boundary must answer with a
token made from Ben's credentials, never Ana's. An agent in Ben's computer is root there and can set
any address it likes.

Measured with two computers played by network namespaces ([the research](../../../research/2026-11-25-shared-linux-host/README.md), `net.sh`):

| Layout | Ben's computer asks for a token | With Ana's computer stopped, Ben takes Ana's address | Ben reaches Ana's computer |
|---|---|---|---|
| One bridge for all computers, the server knows a computer by its address | Ben's token | **Ana's token** | yes |
| The same bridge, each port held to its own address and MAC, ports isolated | Ben's token | no answer | no |
| A link of its own for each computer, the server knows a computer by the link | Ben's token | Ben's token | no |

The options:

- A link per computer, the computer known by its link (recommended). Each computer's network stack
  is its own process, or its own link, and the boundary listens on each apart. A computer's address
  means nothing. It costs a listener per computer.
- One shared network, held by filters on each port. It worked here, but every port needs its
  filters, and one missing filter hands out another person's tokens.
- One shared network, the computer known by its address. It costs the case above.

Recommended: a link per computer. It is what a network stack per computer (gvproxy, or Silo's own)
gives already, so it costs nothing on a Mac and keeps the host from needing a bridge.

## A link's name reused, measured

An agent's part, 26 November 2026 ([the research](../../../research/2026-11-25-shared-host-leaks/README.md)). Ana's computer stopped and
its link went; Ben's new computer got a link of the same name. Listeners bound to links, opened
with each link, did not answer it: the kernel binds a listener to a link's index, not its name. A
server restarted that looked each connection's link up by name, in a table not yet rewritten,
gave Ben Ana's token. Rewriting the table with the link closed it. So the answer now says the
owner is written with the link and removed with it. A network stack per computer gives this
already: its process lives and dies with its computer. Cost at 200 computers: 22 ms and 226 KiB of
kernel memory per link, 15.7 MiB for a server with 200 listeners, a token in 63 µs at the median.
