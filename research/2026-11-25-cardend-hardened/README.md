# cardend against a hostile main (25 November 2026)

An agent's work of 25 November 2026, stacked on #402, for the open decision
`way-in-before-the-relay`. Nothing here is a feature polarzero took. Everything ran on Linux, with
no virtual machine and no Mac.

#402 recommends that the Mac end of main's network card, `cardend`, opens the one TCP connection
of the first SSH, with gVisor's netstack behind a filter. Main is where untrusted agents run, and
`cardend` reads every frame main sends, so it faces frames written to hurt it. #402 left it
unfuzzed. This measures what a hostile main can make it do, fixes what it found (each shown
failing first), and fuzzes it.

## What was found

- **A hostile main could wedge the card with its own drop count.** The count (#382's gap 10)
  kept one line per destination address, and main chooses every address. 200,000 frames each to a
  new address left 200,000 lines: 70 to 200 MB of memory and a count file of 6 to 14 MB. The
  count is written every half second under the lock the frame reader takes, so the reader slowed
  from 45,000 frames a second to between 1 and 3,000 within ten seconds and stayed there. The
  SSH of the way in goes through the same reader: its round trip grew from 1 ms to 9.2 s within
  a minute and kept growing. The dead-end card has the same count, so after the relay is in, the
  same flood blinds the trace the place shows.
- **The filter let through more than the one connection.** #402 says the stack "sees only the
  frames of that connection". It saw any TCP from source port 22 to the Mac end: a SYN to any
  port, a segment from any 169.254 address, IP fragments (a later fragment whose data starts with
  `00 16` reads as port 22), IP options, frames of 64 KB, and ARP for the Mac end from any
  address. No listener was ever open, so main could not open a connection, but it could make the
  stack talk: each segment from a made-up 169.254 address made it send main an ARP request, one
  frame out per frame in, at 45 µs of CPU each.
- **Fixed, the floods do nothing.** After the fix, in every flood below, nothing reached the stack,
  the card sent main nothing, its memory stayed at 14 MB, the count stayed at 257 lines and under
  18 KB, and the frame reader kept 270,000 to 280,000 frames a second for 90 s. The SSH round trip
  under the same flood stayed at 2 ms or less for a minute. The way in still copies 2 MB intact.
- **Fuzzed, no crash.** Go's fuzzer sent 3.5 million arbitrary frames to the card (150 s), and
  9,535 runs of arbitrary TCP segments on the open connection's own addresses and ports, while Silo
  writes to it, against gVisor's TCP (300 s). Neither panicked, and the card never sent anything
  but ARP or TCP from its address to main's. #402's card already passed the second fuzz for 120 s
  (29,854 runs): gVisor's TCP held as the peer of a hostile main.
- **What a hostile main can still do: keep one core busy.** Each frame costs the card about 3.5 µs
  (92 s of CPU for 25.5 million frames in 90 s). The frames are read from a socket main fills as
  fast as it likes, so one core of the Mac goes to reading them while main floods. Bounding that
  needs the Mac's side measured: whether Apple's end of the socket pair drops frames when the card
  reads slowly, or makes main wait. Not built.

## What the fix is

`cardend/main.go` in #402's folder, about 110 lines more, in one commit after the one with the
tests:

- **The count holds at most 256 lines.** Past that, a frame to a new address is counted on its
  kind's line "other addresses" ("tcp connect, other addresses ports 443,... x150000"). No frame is
  lost from the totals.
- **The stack sees only the open connections.** ARP whose sender is main's learned address and
  whose target is the Mac end's; TCP from main's learned address, port 22, to a port the Mac end
  has a connection open from; unfragmented, with no IP options; frames of at most 1,514 bytes.
  Before main's address is learned, nothing. The Mac end chooses each connection's local port
  itself and opens it in the filter before the SYN leaves, and closes it with the connection.
- **The stack can send to main only.** Its address is a /32 and its one route, added when main's
  address is learned, goes to main's address alone. Before, it had all of 169.254.0.0/16, which is
  why the made-up sources got ARP requests.
- **No SACK.** The card loses no frames, so SACK only adds code main's segments reach.

The frame handling moved into a `card` type first, with no change of behaviour (the first commit),
so tests can drive it.

## Measured

Each flood: 200,000 frames (20,000 for 64 KB frames) as fast as the socket takes them, against a
fresh `cardend` in way-in mode that has learned main's address, then 2.6 s for the card to catch up.
The driver is [`hostile/`](hostile/); results in [`results/`](results/).

| Flood | Before: memory | Before: count lines, file | Before: frames sent back | After: memory | After: count lines, file | After: sent back |
|---|---|---|---|---|---|---|
| one frame again and again | 14 MB | 2, 68 B | 0 | 14 MB | 2, 68 B | 0 |
| ARP for a new address each | 69 MB | 199,996, 6.5 MB | 0 | 14 MB | 257, 8 KB | 0 |
| SYN to a new address each | 149 MB | 199,996, 8.8 MB | 0 | 14 MB | 257, 11 KB | 0 |
| ping to a new address each | 73 MB | 199,996, 5.1 MB | 0 | 14 MB | 257, 7 KB | 0 |
| SYN to a new IPv6 address each | 200 MB | 200,001, 13.7 MB | 0 | 14 MB | 257, 18 KB | 0 |
| SYN from port 22 to each port of the Mac end | 102 MB | 2 | 3 | 14 MB | 2 | 0 |
| TCP from port 22 of a new 169.254 address each | 15 MB | 2 | 197,839 (ARP requests, 8.3 MB) | 14 MB | 2 | 0 |
| first fragments that never complete | 28 MB | 2 | 0 | 14 MB | 2 | 0 |
| ARP replies naming a new neighbour each | 14 MB | 3 | 0 | 14 MB | 3 | 0 |
| 64 KB frames from port 22 | 26 MB | 2 | 3 | 12 MB | 2 | 0 |

Over time, a flood to a new IPv6 address each for 90 s
([before](results/before-rate-v6-90s.jsonl), [after](results/after-rate-v6-90s.jsonl)): before,
45,000 frames a second for the first 5 s, then mostly 1 to 8 a second, 284 MB and 105 s of CPU;
after, 270,000 to 280,000 a second throughout, 14 MB.

The SSH round trip of 1 KB through the way in under that flood, for a minute, with main a real
gVisor stack echoing on port 22 and one frame reader as in `cardend`
([before](results/before-ssh-under-flood.jsonl), [after](results/after-ssh-under-flood.jsonl)):
before, up to 9.2 s, rising with the count (3.2 million lines at the end); after, at most 2 ms.

Why the SYNs from port 22 cost #402's card 102 MB with only 3 frames back was not traced: the
stack answers each with a reset that waits for main's ARP reply, which the driver never sends.

## Tests and fuzz

In [`../2026-11-25-way-in-before-the-relay/cardend/`](../2026-11-25-way-in-before-the-relay/cardend/):

| Test | On #402's card | After |
|---|---|---|
| `TestWayInEndToEnd`: 2 MB both ways through the way in to a real gVisor main | pass | pass |
| `TestCountsStayBounded`: 150,000 frames to new addresses | fail: 150,001 lines, 5 MB | pass |
| `TestStackSeesOnlyItsConnection`: 7 kinds of frame the way in does not need | fail: 7 of 7 reach the stack | pass |
| `TestNothingSentForStrayFrames`: 2,000 stray frames | fail: 1,001 frames sent | pass |
| `FuzzFrame`: any frame; nothing sent but ARP or TCP to main, count under 300 lines | fails on the count | 3.5 M runs, pass |
| `FuzzConnection`: any TCP segments on the open connection | 29,854 runs, pass | 9,535 runs, pass |
| `TestMeasureWayInUnderFlood`, with `CARDEND_MEASURE=<s>` | a measurement | a measurement |

## What this changes in the plan

- `way-in-before-the-relay`: a note under its own heading with the findings. The recommendation
  stands; the program is about 110 lines longer. No verdict.
- `network-place`: a note that the count is bounded, so the place reads "to more than 250
  addresses" past that.
- No other entry.

## Not verified

- Nothing ran on a Mac. The card read frames from a Unix datagram socket, as #402's stand-in does;
  whether Apple's `VZFileHandleNetworkDeviceAttachment` drops frames or makes main wait when the
  card reads slowly was not found, and it decides what a flood costs the Mac.
- The stand-in listens on a socket path and sends to whoever wrote last, so another process of
  the Mac could take main's place. On a Mac the card gets one end of a socket pair from Silo and
  has no path. Not shown failing, so not changed; the first build should take the socket as an
  inherited descriptor.
- The fuzz ran for minutes, not days, on 2 workers. The connection fuzz runs 70 to 200 inputs a
  second, since each builds a stack and a connection.
- gVisor is the version #402 pinned (`2823bce7c7de`). Its fragment reassembly, which #402's card
  reached and this one no longer does, was not fuzzed.
- Each flood ran once, on a 4-core container.

## Run it

```
(cd research/2026-11-25-way-in-before-the-relay/cardend && go test . && go build -o /tmp/cardend .)
(cd research/2026-11-25-cardend-hardened/hostile && go build -o /tmp/hostile .)
research/2026-11-25-cardend-hardened/hostile/run.sh /tmp/cardend /tmp/hostile 200000 same arp-spread tcp-spread icmp-spread v6-spread syn22 spoof22 frag22 arp-reply-spread
research/2026-11-25-cardend-hardened/hostile/rate.sh /tmp/cardend /tmp/hostile v6-spread 90s
(cd research/2026-11-25-way-in-before-the-relay/cardend && go test -run XXX -fuzz FuzzFrame -fuzztime 150s .)
(cd research/2026-11-25-way-in-before-the-relay/cardend && go test -run XXX -fuzz FuzzConnection -fuzztime 300s .)
```

Needs Go 1.27 and Python 3. No root.
