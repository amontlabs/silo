# How the first run reaches a new macOS computer before Silo's relay is in it

## The choice

An agent's part, 25 November 2026, from `research/2026-11-25-first-run-option-a/` (gap 1 there).
#378 proposes a network card for main that leads nowhere: its other end, on the Mac, answers ARP
for one router address and drops every other frame. The only way out is a proxy in main at
`127.0.0.1:3128`, carried over vsock to Silo on the Mac, and the way in is SSH carried over vsock
by the same relay. Apple's provisioning (`mac-silo-needs`) creates the user and turns on Remote
Login, which listens on the network card. On a card that leads nowhere, the first SSH has no road,
and the relay that would carry it must be installed over SSH.

The options, each run on Linux in `research/2026-11-25-way-in-before-the-relay/` (a network
namespace stood for main, with sshd on port 22 as provisioning leaves it, and a service on the
Mac's loopback, one on every address of the Mac, a neighbour on the local network and an internet
host around it):

- The Mac end of the card makes the one connection (recommended). The program on the Mac end of
  #378's card, which answers ARP for one router address and drops the rest, also opens one TCP
  connection to main's port 22 when Silo asks. Main finds no DHCP server, so macOS gives itself a
  169.254 address and announces it by ARP; the Mac end learns it from that frame. The TCP code is
  gVisor's netstack, the one gvproxy uses, and it sees only the frames of that connection: every
  other frame is dropped and counted before it. Measured: SSH got in and copied 2 MB intact; from
  inside, main reached nothing (13 of 13 cases blocked, no name lookup, no port open on the Mac end
  in a scan of all 65,535). It keeps `network-stack`'s answer, needs neither Apple's NAT nor gvproxy
  nor test 07, and needs no second start, since the card stays the same. It costs a program of
  about 370 lines in Go with gVisor's netstack, and three Mac facts not checked yet: that a
  provisioned macOS 27 main gives itself a 169.254 address on a card with no DHCP, how long that
  takes, and that Remote Login listens on it.
- Apple's NAT for the first start only. Main starts on Apple's NAT, Silo connects over SSH, installs
  the relay and writes the proxy settings, then stops main and starts it again on the dead-end
  card. Measured on a stand-in for Apple's NAT: during that start, main reached the Mac's own
  addresses and the services listening on them, every machine of the local network (TCP, UDP and
  ping), the internet, and the Mac's resolver, which saw a made-up name looked up from inside. It
  costs one more start and goes against `network-stack`'s "Never Apple's NAT", so the record says
  so. Only Apple's setup and Silo's own steps run then.
- gvproxy with `-closed` (#380) for the first start only. Measured: main reached nothing, SSH got in
  through the forward on gvproxy's socket, and gvproxy's DNS port 53 stayed open on the gateway,
  refusing every name. It keeps `network-stack`'s answer and costs a second start, gvproxy in the
  first build, and #359's test 07 as a blocker, which #378 freed option A from.
- The relay written onto main's disk before its first start, as the open-source tool cove does with
  its vsock agent. Nothing would ever reach main over a network card. It goes against
  `mac-silo-needs`' "nothing written to its disk from outside", and Lume starts a computer once
  before writing, so it may still need one of the ways above. Not run.
- The serial console. A Linux main gets a shell there from its kernel's command line, and 2 MB
  went through it intact in 37 s with no network card at all. A macOS computer runs no login on its
  console (`/dev/cu.virtio`) unless something writes that setting first, so it is no way into a
  macOS main.

Recommended: the Mac end of the card makes the one connection. It is the only option that keeps
`network-stack`'s answer without a second component, and the program is the dead-end card's,
which the first build needs anyway. If the Mac test shows main does not take a 169.254 address in
time, Apple's NAT for the first start is the fallback.

What is not known: whether Apple's provisioning can start a program given in its options, which
would let the relay start with no SSH at all. Neither #352 file 10, #378 nor this research found
such an option.

Once the relay is in, Silo reaches main only through it: each vsock connection gets its own
`sshd -i`, so Remote Login on the card can be turned off and main listens on no TCP port for
SSH. Measured on Linux with a Unix socket standing in for vsock.

## Where the drafts of 25 November disagreed (integration)

An agent's part, 25 November 2026, written while merging every draft into one branch (`research/2026-11-25-integration-2/`).

#380 measured the second option of the choice above on Linux: gvproxy with its `-closed` flag reached nothing from inside the computer, and SSH still got in. That makes the option that keeps `network-stack`'s "never Apple's NAT" measured, though not on a Mac. Recommended: keep Apple's NAT for the first start, as above, until test 07 has run on a Mac. If test 07 passes with #380's patch, take the gvproxy option, so no packet of main ever goes through Apple's NAT.

## The ways in, measured

An agent's part, 25 November 2026, from `research/2026-11-25-way-in-before-the-relay/`. The choice
above now holds the measured options, and its recommendation replaces the one in the section
before this: the Mac end of the card makes the one connection, so main is never on Apple's NAT and
gvproxy is not needed for the first start either. The section before stays as the integration
wrote it.

## The Mac end of the card against a hostile main

An agent's part, 25 November 2026, from `research/2026-11-25-cardend-hardened/`. The recommended
option puts a program on the Mac that reads every frame main sends. Main is where untrusted agents
run, so that program faces frames written to hurt it. Measured on Linux against #402's program, and
fixed:

- Its count of dropped frames kept one line per destination address. Main chooses every address,
  so a flood of frames each to a new address grew the program's memory without end (200,000
  frames: 200 MB, a 14 MB count file) and the count written every half second slowed it until it
  handled a few frames a second. The SSH of the way in shares that reader: its round trip grew
  from 1 ms to 9 s within a minute. Now the count holds at most 256 lines; past that, a new
  address goes on its kind's "other addresses" line, and no frame is lost from the totals.
- Its filter let through more than the one connection: a SYN from port 22 to any port, TCP from
  any 169.254 address, IP fragments, IP options, 64 KB frames and ARP from any address. Each
  spoofed source made the netstack send main an ARP request, one frame out per frame in. Now the
  stack sees only ARP between main's learned address and the Mac end's, and TCP from main's port
  22 to a port the Mac end has a connection open from, unfragmented, without options, of at most
  1,514 bytes; it has a route to main's address only, and no SACK.

After the fix, nothing main sent in any of the measured floods reached the stack or made the card
send a frame, its memory stayed flat, and the way in still copied 2 MB intact. Fuzzed with Go's
fuzzer: any frame against the card, and any TCP segment main can send on the open connection
against gVisor's TCP. No crash was found in gVisor. What main can still do is keep one core of the
Mac busy by sending frames as fast as the socket takes them, about 3 µs each; the Mac's side of the
socket pair was not measured. This changes no option above: the recommended one stands, with the
program grown by about 110 lines.

## Settled in the consolidation (27 November 2026)

Settled, 27 November 2026, in the consolidation: the recommendation above is out of date. #402 measured the card that leads only to Silo, and the answer now takes it: no packet of main goes through Apple's NAT, even at its first start. The Mac kit's test 43 checks it on a Mac.

## More research on this (27 November 2026)

An agent's part, 27 November 2026. These reports bear on this entry, and its notes did not point to them. The answer is unchanged.

- `research/2026-11-25-red-team-round-two/` (#435), finding 2: the Mac end of the card kept a count with no bound, so a scan from main filled the Mac's memory and disk. The report also shows that a SYN from main does reach the Mac end's stack, against what the draft said. Its patch fixes both, with tests that fail first.
- `research/2026-11-26-headline-numbers/` (#439): the way in was run again and gave the same results. The copy of 2 MB took 0.23 s, 35 s and 12 s on three runs, so its speed is not an argument until it is measured over several runs.
