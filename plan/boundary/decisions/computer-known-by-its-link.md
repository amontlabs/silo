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
