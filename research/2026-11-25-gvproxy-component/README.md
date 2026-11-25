# gvproxy as a part of Silo (25 November 2026)

An agent's work of 25 November 2026, for the open decision `network-stack`. Nothing here is a
feature polarzero took. It continues three drafts: #358 found that code inside a computer opens
ports on the Mac through gvproxy's API, #363 wrote a 64-line patch (`-sealed`) against it, and #366
measured what a user-space stack costs. Everything was run on Linux, in network namespaces, with no
virtual machine and no Mac.

## What was found

- **#363's patch leaves ping open.** gvproxy 0.9.0 (7 October 2026) added ICMP forwarding, and the
  patch filters only TCP and UDP. With `-sealed` on today's gvproxy, a computer still pings your
  Mac's own address and every machine on your local network. On a Mac this works without root,
  because macOS lets any user open an ICMP socket. That is enough to find every machine on your
  network from inside a computer.
- **gvproxy's API on the computer's gateway is still on by default upstream.** CVE-2026-107935
  (published 9 October 2026, CVSS 9.3 per Endor Labs, Important per Red Hat) let code in the
  computer delete any file of the gvproxy user on the host through that API. 0.9.0 fixed the file
  deletion, not the API: the address `192.168.127.1:80` still opens ports on the host unless a
  token is set, as #358 showed.
- **A patch of about 320 lines (and 180 lines of tests) makes gvproxy fit Silo.** It adds five
  options, each useful outside Silo, so each can be proposed upstream on its own. With them, a
  computer reaches nothing of your Mac or your network in any of 21 hostile cases, a closed computer
  reaches nothing at all while SSH still gets in, and every TCP connection can be handed to Silo's
  boundary so that gvproxy itself dials nothing.
- **The options cost nothing measurable.** Throughput, round trip and new connections per second
  stay within the run-to-run noise of stock gvproxy. Handing connections to a stand-in boundary adds
  about 0.1 ms to a round trip and keeps the throughput.
- **The datagram socket a Mac uses is slower into the computer.** Apple's framework hands frames to
  gvproxy one per datagram. Over that socket, at MTU 1500, traffic into the computer was 0.28 to
  0.90 Gbit/s, against 1.05 to 1.13 over a stream socket. See "Throughput".
- **The recommendation:** the first build takes #378's card that leads nowhere, with a proxy, for a
  closed or limited main; the boundary takes connections both from that proxy and from gvproxy's
  hand-off; gvproxy with this patch carries an open main later; Silo does not write its own TCP/IP
  now. Details, and the cases that would change it, are in "Fork, upstream, Silo's own stack, or no
  stack".

## The patch

[`gvproxy-silo.patch`](gvproxy-silo.patch) applies to upstream
`containers/gvisor-tap-vsock` at commit `197eea05` (25 November 2026, 0.9.0 and 52 commits).

| Option | What it does | Why Silo needs it |
|---|---|---|
| `-no-gateway-api` | gvproxy serves its HTTP API only on the sockets Silo opens on the Mac, never on `192.168.127.1:80` inside the computer | #358: one `curl` from inside opened a port on every interface of the Mac |
| `-no-host-loopback` | No address inside the computer leads to the Mac's `127.0.0.1` (the default maps `192.168.127.254` there) | #358: six services on the Mac's loopback were reached |
| `-outbound-deny <list>` | Destinations no TCP connection, UDP datagram or ping may reach, checked after address translation. Entries are addresses, CIDRs, `private`, `host` (the Mac's own addresses, read again each second) or `all` | The local network, the Mac's own addresses, cloud metadata, and the ping that #363 missed |
| `-dns-upstream none` or `<ip:port>` | Names outside gvproxy's own zones go to that server, or are refused, instead of to the Mac's resolver | A lookup of `<data>.attacker.example` otherwise carries data out through the Mac's resolver, even from a closed computer |
| `-outbound-handoff <socket>` | Every TCP connection is handed to a unix socket, after a PROXY protocol v2 header with the address and port the computer asked for. gvproxy dials nothing; UDP and pings to the outside are dropped | The boundary decides every connection and dials it itself, which `network-modes` asks for: Silo looks the name up and connects to the address it found, whatever address the computer used |
| `-sealed`, `-closed` | Shorthands: `-sealed` is the first two plus `-outbound-deny private,host`; `-closed` adds `-outbound-deny all` and `-dns-upstream none` | The three settings of `network-modes` without a boundary running |

The PROXY protocol header is the one HAProxy defined and many proxies read, so the boundary needs no
format of Silo's for it. The source address in it tells the boundary which computer opened the
connection when one gvproxy serves several.

## The suite

[`suite/`](suite/) runs one gvproxy and one computer that attacks it from inside, then checks from
outside. The computer is a network namespace whose only interface is gvforwarder's tap device, so it
sees what a virtual machine behind gvproxy sees. Around it, the suite places a service on the host's
loopback, a service on all of the host's addresses, a neighbour on the local network at `10.77.0.2`
that also runs the host's DNS resolver, and an internet host at `203.0.113.10`. `runall.sh` runs every
mode and compares each result with [`suite/expected.txt`](suite/expected.txt), which is what Silo
needs. Results are in [`suite/results/`](suite/results/).

| From inside the computer | stock | #363 `-sealed` | `-sealed` | `-closed` | hand-off | hand-off, boundary stopped |
|---|---|---|---|---|---|---|
| Open ports on the gateway | 53, 80 | 53 | 53 | 53 | 53 | 53 |
| Open ports on `192.168.127.254` (the Mac's loopback) | 6 | none | none | none | none | none |
| gvproxy's API: list, and open a port on the Mac | both work | refused | refused | refused | refused | refused |
| A service on the Mac's own address | reached | blocked | blocked | blocked | blocked | blocked |
| A neighbour on the local network, TCP / UDP | reached | blocked | blocked | blocked | blocked | blocked |
| **Ping the neighbour, ping the Mac's own address** | reached | **reached** | blocked | blocked | blocked | blocked |
| Cloud metadata `169.254.169.254`, `0.0.0.1` | blocked | blocked | blocked | blocked | blocked | blocked |
| An internet host, TCP | reached | reached | reached | blocked | reached, by the boundary | blocked |
| An internet host, UDP / ping | reached | reached | reached | blocked | blocked | blocked |
| A name lookup | answered | answered | answered | refused | answered by the boundary | refused |
| Who saw `x<random>.exfil.example` | the Mac's resolver | the Mac's resolver | the Mac's resolver | nobody | the boundary only | nobody |
| A port opened on the Mac from inside | yes | no | no | no | no | no |
| Silo's own way in (a forward made on gvproxy's socket on the Mac, as SSH uses) | works | works | works | works | works | works |

- **The closed computer** that #370 needs is `-closed`: nothing leaves, lookups are refused, and
  SSH still gets in through the forward Silo makes on its own socket. #370's spec says only
  gvproxy can give this; #378 found a second way, with no gvproxy (see below).
- **With the boundary stopped**, gvproxy cannot hand a connection over, so the computer's
  connection is reset and its lookups fail. Nothing goes around the boundary, as `network-stack`'s
  answer asks.
- **`-sealed` still sends lookups to the Mac's resolver**, so a sealed computer without a
  boundary can carry data out in names. That is acceptable only where the computer is open anyway.
- The Go tests in the patch cover the deny list (IPv4-mapped IPv6 included), the
  PROXY header byte for byte, `-dns-upstream` (the Mac's resolver is never asked), and the flags.

The stand-in boundary in [`boundary/`](boundary/) is a test fixture, about 200 lines of Go: it reads
the header, refuses private destinations, dials the rest and answers lookups from a list. Silo's
boundary is the Zig program of #347; reading this header is the one thing it would add.

## Throughput

[`bench/`](bench/) is #366's rig with the internet moved to `203.0.113.10`, since a sealed gvproxy
refuses #366's private address. A namespace stands for the computer; `fwd` carries its frames to
gvproxy over a unix stream socket, as libkrun does, and `fwdgram` over a datagram socket, one frame
per datagram, as Apple's `VZFileHandleNetworkDeviceAttachment` does. iperf3 for 6 s each way, one-byte
round trips, and 300 connections opened one after the other. Median of 3 runs, on a 4-core Linux
container.

| gvproxy | MTU | computer to internet, Gbit/s | internet to computer, Gbit/s | round trip p50 / p99, µs | new connections per s | gvproxy memory, MB | runs |
|---|---|---|---|---|---|---|---|
| stock | 1500 | 1.47 | 1.05 | 212 / 401 | 1423 | 31 | 3 |
| stock | 65520 | 4.03 | 2.77 | 188 / 354 | 1414 | 30 | 3 |
| #363 `-sealed` | 1500 | 1.41 | 1.11 | 240 / 475 | 1313 | 31 | 3 |
| #363 `-sealed` | 65520 | 4.39 | 2.92 | 219 / 391 | 1268 | 30 | 3 |
| `-sealed` | 1500 | 1.43 | 1.11 | 184 / 379 | 1556 | 31 | 3 |
| `-sealed` | 65520 | 4.33 | 3.06 | 225 / 439 | 1346 | 29 | 3 |
| hand-off to the stand-in boundary | 1500 | 1.48 | 1.11 | 291 / 505 | 1405 | 31 | 3 |
| hand-off to the stand-in boundary | 65520 | 3.82 | 3.26 | 302 / 472 | 1329 | 29 | 3 |
| stock, datagram socket | 1500 | 1.11 | 0.38 | 207 / 447 | 1324 | 31 | 3 |
| stock, datagram socket | 65520 | 3.99 | 2.36 | 198 / 373 | 1324 | 29 | 3 |
| hand-off, datagram socket | 1500 | 1.08 | 0.84 | 326 / 620 | 1253 | 31 | 3 |
| hand-off, datagram socket | 65520 | 4.34 | 2.59 | 302 / 533 | 1323 | 29 | 3 |

The raw lines are in [`bench/results/`](bench/results/). The first eight rows use the stream socket.

- **The patch costs nothing measurable.** `-sealed` checks a list on every new connection and reads
  the Mac's addresses at most once a second; #363's patch read them on every connection. Neither
  shows in the numbers.
- **The hand-off costs about 0.1 ms per round trip** (one more unix socket and a relay) and no
  throughput. The boundary's own work, ending TLS and reading requests, comes on top: #347 measured
  no cost on the first byte of a streamed reply, and 34 ms to read a 2 MB request.
- **MTU 65520 triples the throughput** over a stream socket, as #366 found. The computer's network
  device should be set to it from the start.
- **Over the datagram socket, traffic into the computer at MTU 1500 is slow and uneven:** 0.28 to
  0.90 Gbit/s across six runs of stock and hand-off together, where the stream socket gave 1.05 to
  1.13. At MTU 65520 the datagram socket is close to the stream socket. The cause was not found:
  frames lost on a full socket buffer are the likely one, and not shown. A Mac uses this socket, so
  #359's kit should measure it there, at both MTUs.

## Fork, upstream, Silo's own stack, or no stack

The stack has two parts. One turns the computer's Ethernet frames into connections: ARP, DHCP, IP,
TCP, UDP, the lookups. The other, the boundary, decides each connection and reads what it carries.
The boundary is Silo's own in every option below: it is the Zig program of #347, and with the
hand-off it is the only part that dials out. The choice is only about the first part.

#378 (draft, same day) adds a fifth path: no stack at all. The computer's network card leads to a
responder on the Mac that answers ARP for one router address and drops every other frame, and the
only way out is a proxy at `127.0.0.1:3128` inside the computer, carried over vsock to Silo. It ran
on Linux with Claude Code, curl, git, npm and pip.

| | Fork of gvproxy with this patch | The same patch merged upstream | Silo's own program on gVisor's netstack | Silo's own TCP/IP in Zig | No stack: a card that leads nowhere and a proxy (#378) |
|---|---|---|---|---|---|
| Ready | now, measured here | when upstream merges each option | to write: about 3,000 of gvproxy's 7,600 lines do this job | to write | a prototype in Python, measured on Linux |
| Language | Go | Go | Go | Zig, or C through Zig (lwIP, libslirp) | to write; #378's prototype is Python |
| Which programs reach the boundary | every program, with no setting in the computer | the same | the same | the same | only programs that follow proxy settings. Node's `fetch` needs `NODE_USE_ENV_PROXY=1`; any other program gets no network |
| UDP, ping | dropped with the hand-off, or carried under the deny list | the same | as built | as built | none |
| How the boundary learns the host | the address asked for, then the TLS name | the same | the same | the same | the `CONNECT host:port`, before TLS |
| Settings inside the computer | none | none | none | none | system proxy, `HTTP(S)_PROXY`, an SSH `ProxyCommand`, a relay daemon on vsock |
| Throughput | as measured above | the same | the same engine, so about the same | unknown until built | not measured |
| Code Silo keeps | the patch: 320 lines over 12 files | none | about 3,000 lines plus gVisor (92,000 lines, vendored) | a TCP implementation | a small responder and a relay |
| What Silo follows | every upstream change to the files it patches: 47 commits in the last year | releases | gVisor's API, which changes often | nothing outside | each program's proxy support |
| macOS and Linux | both | both | both | both | both (#378 reads that a macOS 27 guest needs the ARP answer, or it stays offline) |

Four facts from this work weigh on the choice:

- **A fork has to follow upstream.** The patch, written against today's main, does not apply to the
  0.9.0 release of seven weeks ago, because `dns.go` changed in between. #363's smaller patch applies
  to both. Each merge of upstream into the fork is small, but it comes with every release.
- **The measured costs come from the TCP engine and the frame transport, not from gvproxy's extra
  features.** The patch removes features and the numbers do not move. A program of Silo's own on the
  same netstack would therefore be no faster; it would only be smaller and Silo's to change.
- **passt is not an option for a Mac.** #366 measured it faster into the computer on Linux, but it
  runs on Linux only; macOS support is an open item on its tracker (bugs.passt.top #6), not a
  release.
- **#378 and the hand-off feed the same boundary.** A `CONNECT` and a PROXY header both give the
  boundary a destination and a stream. The boundary can take both from the start, and the card can
  later lead to gvproxy instead of the responder without the boundary changing. With the hand-off,
  the boundary also answers the computer's lookups, so it can give each name an address of its own
  (from `198.18.0.0/15`, which is reserved for tests) and know the host from the address before any
  TLS, as a `CONNECT` tells it. That is not built here.

**Recommended:**

1. For the first build, #378's card that leads nowhere, for a closed main and a main limited to a
   list. It needs no gvproxy and does not wait on #359's test 07. A program that ignores proxy
   settings gets no network at all, which is the safe way to fail.
2. Make the boundary take both a `CONNECT` and a PROXY v2 hand-off from its first version. It is a
   few dozen lines in the reader of #347.
3. When main should be open with rules and every program should work in it unchanged, put the card
   on gvproxy with this patch and the hand-off. Run that fork pinned to an upstream commit, with
   the suite and the Go tests run on every merge of upstream, and propose the five options upstream
   as five separate pull requests, so the fork ends if they merge.
4. Do not write Silo's own TCP/IP now. Write a program of Silo's own only on a measured need: two
   computers that exchange data through the boundary too slowly (#366's case), or a Mac datagram
   transport that stays the limit and that gvproxy cannot fix. The suite and the bench here are what
   it would have to pass.

Proposing the options upstream is public and names the problem #358 found. Whether to do it, and
whether to tell gvproxy's maintainers privately first that the gateway API is on by default, is
polarzero's call.

## What this changes in the plan

- `network-stack`: notes added, with the numbers and the recommendation above. Its answer does not
  change: every packet goes through Silo's stack, and if the stack stops the computer has no
  network. The suite shows the second sentence holds with the hand-off.
- `network-before-rules` (#348) says an open main "reaches the internet but never your Mac, your
  local network or cloud metadata". Of gvproxy's builds, only this patch holds that: stock gvproxy
  and #363's patch fail on ping, and Apple's NAT fails on all of it (#370). #378's card holds it too,
  as long as its proxy refuses those addresses.
- `nothing-inside-reaches-the-stack` (proposed property, #358): the suite is its check. The gateway
  keeps one service, the DNS server on port 53; with the hand-off, its answers come from the boundary.
- #370's step 2: "Patched gvproxy (once test 07 passes)" can name this patch and its flags: `-closed`
  for a closed main, the hand-off once the boundary exists, `-sealed` in between.
- #359's Mac kit, test 07: add a ping to the Mac's address and to a machine on the local network,
  and a lookup of a random name watched at the Mac's resolver. Those are the two paths a Linux run
  cannot show on Apple's own stack.

## Not verified

- Nothing ran on a Mac. Three things need one: that unprivileged ping forwarding behaves on macOS
  as on Linux (macOS allows it by design; not run here), the datagram socket's size limits there
  (`net.local.dgram.maxdgram` is 2 KB by default, gvproxy raises its buffer to 1 MB) and the largest
  MTU `VZFileHandleNetworkDeviceAttachment` accepts.
- No virtual machine ran: a network namespace stood for the computer. A guest kernel adds its own
  costs on both sides equally.
- IPv6 inside the computer was not tested: gvproxy gives the computer IPv4 only. The deny list
  handles IPv6 addresses for when it does.
- The stand-in boundary is not Silo's. It was not measured with TLS or HTTP reading.
- Switching a running computer between settings was not built. gvproxy reads its flags at start,
  so with `-sealed` or `-closed` a switch means restarting gvproxy, which drops the computer's
  connections. With the hand-off, the switch is the boundary's, at any time, and can cut the
  connections already open, as `network-modes` asks.
- Whether upstream would merge the options was not asked.
