# Texts for containers/gvisor-tap-vsock, ready to send

Nothing here has been sent. Sending is public, so it is polarzero's call. Each pull request is one
patch of [`patches/`](patches/); its description is the patch's commit message, which is written
for upstream. Below are the issue that opens the conversation, the order to send in, and a short
cover for each pull request.

Before sending:

- **Sign-off.** Upstream's commits carry `Signed-off-by:` (the Developer Certificate of Origin, as
  the containers projects ask). The patches carry none and name `silo-research` as author: apply
  them with `git am --signoff` and `git commit --amend --reset-author` under the name that sends
  them. Only the person sending can certify the DCO.
- **Base.** `patches/on-main/` (five patches) applies to upstream `main` at `197eea05` alone.
  `patches/on-750/` (nine for upstream, one for Silo's fork) applies on upstream PR #750, "Add a
  dial filter and a DNS answer observer for embedders", which is approved and not merged. Re-run
  [`check.sh`](check.sh) on the day of sending: it applies every patch, builds for Linux, macOS
  and Windows, runs the tests after each one, and runs upstream's `make lint`.
- **Security.** Upstream's policy (containers/common `SECURITY.md`) asks for vulnerabilities to go
  to `security@lists.podman.io`, never to a public issue. None of the patches discloses something
  new about the gateway API: upstream's README already says it is unauthenticated unless
  `--api-token-file` is set. The three bugs fixed here are each small: a flag that does nothing, a
  ping that goes where TCP may not, and a connection to the gateway's address that leaves for the
  host's network. The third can reach a machine of the user's own network that holds
  192.168.127.1. Whether to send that one privately first is in the README's "For you to rule".

## Order

| # | Pull request | Kind | Needs | Patch |
|---|---|---|---|---|
| 1 | gvproxy: pass -ec2-metadata-access to the virtual network | bug fix | nothing | on-main 0001 |
| 2 | forwarder: do not forward pings to link-local addresses | bug fix | nothing | on-main 0004 |
| 3 | gvproxy: add -no-gateway-api | option | nothing | on-main 0002 |
| 4 | gvproxy: add -no-host-loopback | option | nothing | on-main 0003 |
| 5 | gvproxy: add -dns-upstream | option | nothing | on-main 0005 |
| 6 | virtualnetwork: do not forward connections to the gateway's own address | bug fix | #750 | on-750 0006 |
| 7 | gvproxy: add -outbound-deny | option | #750 | on-750 0007 |
| 8 | forwarder: let an embedder open the host side of TCP connections | API | #750 | on-750 0008 |
| 9 | gvproxy: add -outbound-handoff | option | 8 | on-750 0009 |

Send 1 and 2 at once: they are bug fixes with a test each and need no discussion. Open the issue
below next and send 3 to 5 with it. Send 6 to 9 once #750 merges, or as a comment on #750 before
that, since 6 and 7 are the first users of its filter. If #750 changes before merging, 2 and 5 on
`on-750` differ from `on-main` only in the lines #750 also touches.

## The issue

**Title:** Options to keep a guest away from the host and its network

> gvproxy is used more and more to run code that is not trusted: coding agents, CI jobs, sandboxes
> (#609, #630, #631, #687 and #750 all come from that). For them, what the guest can reach on the
> host matters as much as what it can reach on the internet. Today, with default options, a guest:
>
> 1. uses the HTTP API on 192.168.127.1:80, which can expose a port of the host on all of its
>    interfaces, unless every user sets `--api-token-file` and keeps the token from the guest's own
>    processes;
> 2. reaches every service on the host's 127.0.0.1 through 192.168.127.254, and the configuration
>    file cannot turn that off (an empty `stack.nat` is replaced by the default);
> 3. reaches the host's other addresses and every machine of the host's local network, by TCP, UDP
>    and, since 0.9.0, ping;
> 4. resolves any name through the host's resolver, so a guest that can connect nowhere can still
>    send data out in names.
>
> We would like to propose small, separate options, each off by default and each useful alone:
>
> - `-no-gateway-api`: do not serve the API inside the network (it stays on `--listen` and
>   `--services`);
> - `-no-host-loopback`: no default NAT entry to 127.0.0.1;
> - `-dns-upstream ip:port|none`: send the names outside gvproxy's zones to a given server, or
>   refuse them;
> - `-outbound-deny`: a deny list (`private`, `host`, `host-networks`, `all`, addresses, CIDRs)
>   built on #750's dial filter;
> - `-outbound-handoff <socket>`: hand every TCP connection to a unix socket after a PROXY v2
>   header and dial nothing, for users who run their own egress proxy, built on a small `DialTCP`
>   hook next to #750's filter.
>
> While testing them we found three bugs, with fixes and tests in separate pull requests:
> `-ec2-metadata-access` has done nothing since the configuration file was introduced (#484,
> v0.8.8); the ICMP forwarder (#597) forwards pings to 169.254.0.0/16, which TCP and UDP refuse;
> and a connection to a port of the gateway where gvproxy serves nothing is forwarded to the same
> address on the host's network.
>
> Each patch builds on Linux, macOS and Windows, passes `make lint`, and comes with unit tests. We
> also ran them in a Linux harness where a network namespace plays the guest behind gvforwarder,
> with services on the host's loopback, on its local network, on a local network with public
> addresses and on a stand-in metadata address, and an internet host behind a router; we can share
> it. Would you take these as separate pull requests, and do the names suit you?

## Cover notes for each pull request

Each description is the commit message. Add on top:

1. **ec2 flag.** "Regression from #484. The test fails on main with `expected: true, actual:
   false` for the flag and for the top-level key." Mention #512 and #647 (dornseifer, DTrippe),
   whose users this affects.
2. **ICMP link-local.** "Follows #597 and #647. The new test maps 169.254.169.254 to the host's
   loopback, so an echo comes back whenever the forwarder sends the ping; it fails on main."
   Note that the other early returns of the ICMP handler (no packet, not an echo, and #750's
   filter) do not release the packet the handler holds; this patch releases it on its own path
   only, and a follow-up can do the rest.
3. **-no-gateway-api.** "Complements #724's token: for users who never call the API from the
   guest." Names are open to change.
4. **-no-host-loopback.** "A NAT entry of the configuration file that leads to a loopback address is
   refused with this option, so the option means what it says. Say if you would rather keep such
   entries."
5. **-dns-upstream.** "The configured servers answer through the same code paths as the host's
   resolver, so answers keep the shape they have today, and #750's observer still sees them."
6. **Gateway's own address.** "Found while testing -no-gateway-api: with the API off, port 80 of
   the gateway behaves like any other port where gvproxy serves nothing, and is forwarded."
   Built on #750's filter because that is the one place every forwarder asks before dialling. A
   version without #750 would add the gateway address to the TCP and UDP forwarders' arguments.
7. **-outbound-deny.** Point at #630 (deny-all policy) and #687 and #609 (allow lists by name):
   this is the address-level floor those can sit on, and it composes with #750's filter instead
   of adding a second hook.
8. **DialTCP.** "The source is a separate argument, so `DialRequest` and every filter written for
   #750 stay as they are."
9. **-outbound-handoff.** "PROXY v2 because HAProxy, Envoy, nginx and go-proxyproto read it. UDP
   and ICMP are dropped in this mode."
