# The way into main before the relay (25 November 2026)

An agent's work of 25 November 2026, for the open decision `way-in-before-the-relay`, which #382
opened (gap 1 there). Nothing here is a feature polarzero took. Everything ran on Linux, in
network namespaces, with no virtual machine and no Mac.

The problem, in one case. You run Silo for the first time. Apple's provisioning creates main's user
and turns on Remote Login. Under #378's network, main's only network card leads to a program on the
Mac that answers ARP for one router address and drops every other frame, and main reaches the world
only through a relay over vsock. That relay is not in main yet, and Silo installs it over SSH. So
the first SSH needs a road that the card does not give.

## What was found

- **The Mac end of the card can make the one connection itself, and main reaches nothing while it
  does.** [`cardend/`](cardend/) is #378's dead-end card with one addition: on Silo's request it
  opens one TCP connection to main's port 22. Main gets no address from anyone, so it gives itself
  a 169.254 address and announces it by ARP, as macOS does when no DHCP server answers; `cardend`
  learns the address from that frame. The TCP code is gVisor's netstack, the one gvproxy uses, and
  it sees only the frames of that connection. Every other frame is dropped and counted before it.
  SSH got in and copied 2 MB intact. From inside, 13 of 13 attempts to reach something were
  blocked, no name resolved, and a scan of all 65,535 ports of the Mac end found none open.
- **Apple's NAT opens everything the plan keeps from main, for as long as the first start lasts.**
  On a stand-in (the kernel's NAT, with the Mac as router and resolver, as Apple's is), main
  reached the Mac's own addresses and every service listening on them, a machine on the local
  network by TCP, UDP and ping, the internet, and the Mac's resolver, which saw a made-up name looked
  up from inside.
- **gvproxy with `-closed` (#380) also opens nothing,** except its DNS port 53 on the gateway, which
  refuses every name. It needs gvproxy in the first build and #359's test 07 before the first run.
- **Once the relay is in, main needs no SSH port.** The relay starts `sshd -i` for each vsock
  connection, so Remote Login on the card can be turned off. Measured with a Unix socket standing
  in for vsock.
- **The serial console is a way into a Linux main, not a macOS one.** With no network card at all,
  2 MB went through a console shell intact in 37 s. A macOS computer runs no login on its console
  unless a setting is written first.
- **The dead-end card's drop count works** (#382's gap 10). A program that ignored the proxy shows
  up as lines like "tcp connect 10.77.0.2 ports 7778"; a scan is one line.

Recommended for `way-in-before-the-relay`: the Mac end of the card makes the one connection.
It keeps `network-stack`'s "Never Apple's NAT", needs neither gvproxy nor test 07, needs no second
start of main, and is the dead-end card's own program, which the first build needs anyway. Apple's
NAT for the first start is the fallback if the Mac test below fails.

## The ways in, side by side

[`ways/ways.sh`](ways/ways.sh) runs each way the same way. Main is a network namespace with sshd
on port 22 and Silo's key authorised, as provisioning leaves it. Its card is a tap device whose
frames go one per datagram over a Unix socket (#380's `fwdgram`), as Apple's
`VZFileHandleNetworkDeviceAttachment` hands them over, except for Apple's NAT, where a veth pair
stands in. Around main, #380's suite places a service on the Mac's loopback, one on every address
of the Mac, a neighbour on the local network at `10.77.0.2` that runs the Mac's resolver, and an
internet host at `203.0.113.10`. From inside, [`ways/probe.py`](ways/probe.py) runs 13 of #380's
cases (its four gvproxy-only cases left out) and scans every port of the address the way gives
main as its router. From the Mac, Silo's ssh connects, runs one command and copies 2 MB, the size
of a relay. Results of one run each: [`ways/results/`](ways/results/).

| | Apple's NAT | gvproxy `-closed` | Mac end of the card, one connection | After: the relay over vsock | Serial console, no card |
|---|---|---|---|---|---|
| SSH gets in | yes | yes, by a forward on gvproxy's socket | yes, by `cardend`'s socket | yes, `sshd -i` per connection | a shell, no SSH |
| 2 MB copied | intact, 0.27 s | intact, 0.24 s | intact, 0.26 s | intact, 0.14 s | intact, 37 s |
| The Mac's own address, TCP and ping | reached | blocked | blocked | blocked | blocked |
| The Mac's loopback | blocked | blocked | blocked | blocked | blocked |
| A neighbour on the local network: TCP, UDP, ping | reached | blocked | blocked | blocked | blocked |
| Cloud metadata, `0.0.0.1` | blocked | blocked | blocked | blocked | blocked |
| The internet: TCP, UDP, ping | reached | blocked | blocked | blocked | blocked |
| A name lookup | answered | refused | no name server | no name server | no name server |
| Who saw a made-up name looked up | the Mac's resolver | nobody | nobody | nobody | nobody |
| Ports open on main's router | every port the Mac listens on (2 here) | 53 | none | none | no router |
| A second start of main | yes | yes | no | | |
| New in the first build | nothing | gvproxy with #380's patch, test 07 | none: the dead-end card's program | | a console in the computer's settings |
| Works for a macOS main | yes | if test 07 passes | if main takes a 169.254 address (not checked) | yes | no |

The waits to the first SSH in the stand-in (0.3 s to 3 s) say nothing of a Mac and are left out.

### How the Mac end makes one connection

```
 in main                                      on the Mac (cardend)
 ┌───────────────────────────┐                ┌────────────────────────────────────────────┐
 │ sshd on :22               │   datagrams,   │ ARP for 10.254.0.1: answered               │
 │ en0: 169.254.77.7         ├── one frame ───┤ ARP for 169.254.81.16, TCP from :22 ──▶    │
 │ (self-assigned, no DHCP)  │   per datagram │   gVisor netstack ──▶ one TCP connection    │
 │                           │                │   ──▶ Unix socket ◀── Silo's ssh           │
 │                           │                │ every other frame: dropped, counted        │
 └───────────────────────────┘                └────────────────────────────────────────────┘
```

- **Which frames the stack sees.** ARP whose target is the Mac end's own address, and TCP whose
  source port is 22 and whose destination is the Mac end. A SYN from main never reaches it, so
  nothing in main can open a connection, reach a listener or be routed. The scan confirms it.
- **How it finds main.** From the first ARP frame main sends with a 169.254 sender address. macOS
  announces a self-assigned address by ARP (RFC 3927). The stand-in announces with `arping`, since
  Linux does not.
- **Why IPv4.** IPv6 link-local addresses need no DHCP and no wait, and would be the better road,
  but this container's kernel has no IPv6, so only IPv4 was built and run.
- **After the relay.** The same program, with its dial socket closed, is the dead-end card. Silo
  gives main `10.254.0.2` and the proxy settings over the first SSH, starts the relay, and turns
  Remote Login off. Switching the one running program from one mode to the other, with no restart
  of main, was not built: each mode ran as its own process.

### What the dead-end card counted

In the relay run, the probe from inside left these lines in `cardend`'s count (first five ports of
each destination; each SYN sent again counts again):

```
arp who-has router, answered x2
icmp 10.77.0.2 x1                         icmp 203.0.113.10 x1
tcp connect 10.77.0.2 ports 7778,... x3   tcp connect 169.254.169.254 ports 80,... x3
tcp connect 203.0.113.10 ports 8088,... x3
tcp connect 10.254.0.1 ports 1,2,3,4,5,... x65535
udp 8.8.8.8 ports 53,... x6               udp 8.8.4.4 ports 53,... x6
```

The last line is a resolver in the probe falling back to Google's public servers when main has no
name server: a lookup that went around the proxy, which the count shows. In the one-connection
run, main has no route beyond 169.254.0.0/16, so attempts to other addresses fail inside main and
leave no frame; only its ARP for on-link addresses was counted.

That is the trace #382's gap 10 asks for, and the note on `network-place` now says it is
measured. A program that resolves names only through the system's configured servers leaves no frame, since main has none.

## #382's other gaps, against #391's list

| Gap | #391 says | Now |
|---|---|---|
| 1, the first SSH before the relay | open | measured here; recommendation changed in `way-in-before-the-relay` |
| 2, import out of option A | for polarzero | unchanged, for polarzero |
| 3, open or closed in A | closed is possible | unchanged |
| 4, twenty first-seen host alerts | open, note on `alert-only-on-a-change` | still open: it asks to change `new-host-raises-an-alert`, which is in force, so it is polarzero's. #382's transcript already shows the merged line |
| 5, guest clock after sleep | open | taken up by #397 (measured, with a fix) |
| 6, Codex's commands from traffic | open | taken up by the backlog stream "attribution without processes" |
| 7, fewer catches without processes | measured by #387 | unchanged |
| 8, a false «All tests pass.» matched | fixed by #387 | unchanged |
| 9, the only way back | for polarzero | unchanged |
| 10, a program that ignores the proxy | open | closed: built and measured here, note on `network-place` |
| 11, Claude Code's install-script build through the proxy | not run | taken up by the backlog stream on the native build and Codex's WebSocket |
| 12, `codex` named on the first screen | open, one line in #373's `specs.md` | closed: the two lines in `research/2026-11-24-first-build-and-the-record/specs.md` now name no program, with Codex's tool call below as an opinion, as #382's transcript shows it |

## What this changes in the plan

- `way-in-before-the-relay`: the choice rewritten with five measured options; the answer is now
  the Mac end of the card making one connection. A new section says it replaces the integration's
  recommendation. No verdict.
- `network-place`: a note that the drop count was built and measured.
- No other entry. `network-stack`'s answer holds under the recommendation, with nothing to change.

## A challenge

`mac-silo-needs` (open) says "nothing written to its disk from outside". The relay written onto
main's disk before its first start would remove the whole question, as the open-source tool cove
does for its vsock agent. Not recommended now: the one-connection way needs no write, and a write
depends on Apple's files staying where they are. It is listed in the choice so the option is in
front of polarzero.

## Not verified

- Nothing ran on a Mac. Three facts decide the recommendation, and #390's kit should test them on
  the first Mac: that a provisioned macOS 27 main gives itself a 169.254 address on a card with no
  DHCP server, and how long after start; that it announces it by ARP (or sends any frame from it);
  and that Remote Login listens on that address. If any fails, IPv6 link-local is the next road,
  and Apple's NAT for the first start the fallback.
- Whether Setup Assistant needs the internet during provisioning was not found either way. If it
  does, only Apple's NAT works for the first start.
- No virtual machine ran. A namespace stood for main, the kernel's NAT for Apple's, a pty for a
  virtio console and a Unix socket for vsock.
- Each way ran once. The times are of one run on a 4-core container.
- `cardend` was not fuzzed. It hands gVisor's netstack only frames that pass its filter, and its
  own parsing uses gVisor's header readers, but a fuzz of the filter is still to do before it
  faces a computer.

## Run it

```
bash research/2026-11-25-gvproxy-component/build.sh /tmp/wib       # gvproxy-silo, fwdgram
(cd research/2026-11-25-way-in-before-the-relay/cardend && go build -o /tmp/wib/cardend .)
sudo research/2026-11-25-way-in-before-the-relay/ways/ways.sh /tmp/wib nat gvproxy-closed one-connection relay serial
```

Needs root, iproute2, iptables, sshd, socat, arping, curl and Python 3.

## Sources

- The plan at #391's head (`354b89c`).
- #378's `closed-network/` and README, #380's suite and `fwdgram`, #382's README and transcript,
  #352 file 10 (Apple's provisioning, Lume and cove), #391's README.
- Apple's forum thread 758354 (virtio serial ports in a macOS guest appear as `/dev/cu.virtio`);
  cove's documentation (a vsock agent injected at install time); the `vsock(4)` manual page of
  macOS.
