# A computer's network across a restart of gvproxy

Research, 25 November 2026. Backlog item 9 of the day: #390 found on Linux that a restart of gvproxy
kills a computer's network for good when the computer's datagram socket goes straight to gvproxy,
as silovm and vfkit hand it, and that a relay which connects again brought it back. This prototypes
the part of the helper that owns each computer's link, three ways, and runs #390's restart test
against each with more open at the moment of the restart. Linux only, in network namespaces, with
gvproxy v0.9.0 as released (sha256 checked) and a C stand-in for the network card. No virtual
machine and no Mac.

## What was found

1. **Three ways bring the network back, each within 0.1 s of gvproxy being up again.** The
   computer saw about 1.1 s with no network, of which 1 s is the pause this test leaves between the
   crash and the new start. #390's direct link stayed dead in all three runs, and a DHCP renewal
   from inside got no answer.
   - **Re-point**: the helper keeps its own copy of the computer's socket and, once the new gvproxy
     is up, calls `connect()` on it again and says `VFKT`. No copy of any frame, no patch to
     gvproxy, one system call per restart. Possible because a datagram socket may be connected
     again, and the framework's copy of the socket is the same socket.
   - **Hand-off**: the card holds one end of a socketpair, gvproxy is started with the other end
     as an inherited descriptor, and the helper keeps a copy of that end to hand to the next
     gvproxy. A 28-line patch to gvproxy (`gvproxy-fd.patch`: `vfkit: fd://N`). No copy, no
     address for gvproxy to learn, and frames the computer sent while gvproxy was down wait in the
     socket and reach the new gvproxy.
   - **Relay**: the helper moves every frame between the card and gvproxy and connects again after
     a restart (#390's stand-in, here in C). One copy of every frame.
2. **A second `VFKT` cuts a computer off for good, silently.** A 4-byte datagram reaching a gvproxy
   that already has its computer stops gvproxy reading that computer: no error in its log, the
   process keeps running, nothing gets out or in (`short-frame.py`; 6 to 13 bytes and longer did
   not, the cause is not traced). The first version of the relay here connected again on its own
   after a failed send and again on the helper's word, said `VFKT` twice, and lost the computer
   that way. So the helper says `VFKT` exactly once per start of gvproxy, and only the helper
   decides when gvproxy is new. Whether a program inside a computer can send a 4-byte frame
   through Apple's network card, and so cut off its own computer, is a question for a Mac.
3. **Connections open at the restart do not survive it, and the silent ones never end by
   themselves.** gvproxy holds the far half of each connection in its own memory, so a new gvproxy
   knows none of them. Measured in each of the three ways that bring the network back:

   | Open at the restart | Re-point, relay | Hand-off | Relay that sends resets |
   |---|---|---|---|
   | An upload, still sending | reset 0.7 s after gvproxy is up | reset at once | reset at once |
   | A reply held silent, TCP keep-alive after 5 s | reset at its next probe, 1.1 to 1.3 s | reset at once | reset at once |
   | A reply held silent, no keep-alive | still waiting when the test stopped (60 s) | the same | reset at once |
   | A streamed reply, 10 of 60 events received | still waiting at 60 s | the same | reset at once |
   | A session coming in through gvproxy's forward | outside: ended at the crash. Inside: still waiting at 60 s | the same | outside the same, inside reset at once |

   A connection whose program only waits (a model's streamed reply, a held reply, an SSH session
   seen from inside) waits until the program's own timeout, or for ever without one: the computer
   sends nothing on it, so nothing tells it the far end is gone. A connection that sends gets a
   reset from the new gvproxy at its next segment. Under the hand-off, segments queued during the
   restart reach the new gvproxy, so they are reset at once.
4. **Only a helper that reads the frames can end the silent ones.** The relay in `rstrelay.py`
   reads each TCP header it passes, keeps for each open connection the next sequence number the
   computer expects, and once gvproxy is back sends the computer a reset for each, as if from the
   far end. It sent 7 resets per run, and every program inside saw "connection reset" at the moment
   of the re-link and could connect again. That costs a copy of every frame in the helper (speed
   below), and a parser of the computer's frames in the process that holds its secrets.
5. **What else the helper redoes after each start of gvproxy.** gvproxy keeps its forwards in
   memory, so the forward of the computer's SSH (or Silo's way in) is opened again by the helper;
   then a new session in worked at once. The DHCP lease needs nothing: with the computer's address
   as a static lease in gvproxy's configuration, a renewal after the restart got an ACK for the same
   address in every run.

### Speed of each way, no restart

One connection, 5 s each way, three runs, the median, in Gbit/s. "Out" is from the computer to this
machine, "in" the other way.

| Way | Out, MTU 1500 | In, MTU 1500 | Out, MTU 65520 | In, MTU 65520 |
|---|---|---|---|---|
| Direct (#390, no restart possible) | 1.00 | 0.81 | 6.72 | 2.77 |
| Re-point | 0.89 | 0.82 | 6.94 | 2.56 |
| Hand-off | 1.58 | 0.88 | 7.49 | 2.75 |
| Relay (C) | 0.98 | 0.91 | 6.13 | 2.78 |

Runs of one way varied by up to 25% (the relay at MTU 65520: 5.3 to 6.9), so only the hand-off's
gain out at MTU 1500 stands clear of the noise; it held in all three runs (1.44 to 1.60, against
0.96 to 1.00 direct). The relay in C costs nothing measurable at MTU 1500 and perhaps a tenth at
65520: a copy in the helper is cheap, so the case against the relay with resets is its frame parser
in the process holding the secrets, not its speed.

## What it changes in the plan

- `network-stack` (open): notes added. Whichever stack carries a computer, the helper owns the
  computer's end of the link and the stack is the helper's child, restarted by it, one per
  computer. Recommended: the hand-off, on the gvproxy fork #380 already proposes, so a restart costs
  about 0.1 s and no copy, frames sent during it are kept, and it was the fastest way out of the
  computer; re-pointing as the way with no patch if the fork does not happen. The relay with resets
  is the option to take instead if the Mac kit shows that Claude Code or Codex wait long on a cut
  stream. Open
  connections are lost either way, and the silent ones then wait on their program's timeout; a
  stack restart is therefore done outside a night (an upgrade waits for the morning), and a crash
  of the stack in the night is a fact in the record with the connections it cut.
- `processes` (open): a note. Its table puts the network stack inside the helper. With gvproxy the
  stack is a process of its own per computer, which the helper starts; when it stops, the helper
  starts it again and its computer keeps running with a pause of its network.
- `when-the-engine-stops`, `engine-takes-back-helpers`: nothing changes. The link lives in the
  helper, so the engine stopping never touches a computer's network.
- #390's test 36: the outcome `restart-needs-helper` holds on Linux for all three ways, and its
  relay should say `VFKT` only on the helper's word (found 2). New Mac tests are listed below.

## Not verified

- Nothing ran on a Mac. Four questions need one, for #390's kit:
  1. Can the process that hands `VZFileHandleNetworkDeviceAttachment` its socket connect its own
     copy of that socket again (XNU allows `connect()` again on a datagram socket by reading of its
     source, not run), and does the framework keep using it?
  2. Does the framework's network card keep working after its socket returned errors for a second
     (`ECONNREFUSED`, `ENOTCONN` during the restart under re-point), or does it stop the card?
  3. Does it accept a socketpair end (the hand-off) as it accepts a bound socket?
  4. Can root inside a macOS computer send a frame shorter than an Ethernet header through the
     card (found 2)?
- The card is a stand-in in a network namespace: no guest kernel, no virtio queue. The guest's TCP
  is Linux's; macOS's TCP times out and retransmits on its own timings.
- The 1 s pause between the crash and the new start is the test's choice; a helper that watches
  its child would start it again sooner.
- Which agent programs set TCP keep-alive or a read timeout on a model's streamed reply, and so
  how long Claude Code or Codex would wait after a stack restart, was not measured.
- `rstrelay.py` is Python, so its speed is not measured; the relay's cost is relay.c's.
- `gvproxy-fd.patch` was not proposed upstream; that is public, so polarzero's call.

## Files

| File | What it is |
|---|---|
| `link.py` | The helper's part that owns a computer's link, in five ways (`direct` is #390's case), with gvproxy as its child |
| `guest-nic.py`, `nicpump.c` | The stand-in for the network card: DHCP and a TAP device in a namespace, then frames moved in C |
| `relay.c`, `rstrelay.py` | The relay, and the relay that sends resets |
| `gvproxy-fd.patch` | gvproxy v0.9.0 taking its computer's socket as an inherited descriptor |
| `inside.py` | The programs run inside the computer |
| `bench.py` | The restart test and the speed test; `results/` holds their output |
| `short-frame.py` | Found 2, shown |

To run: as root on Linux with `/dev/net/tun`, `unshare` and `nsenter`; build `nicpump` and `relay`
with `gcc -O2` into `/tmp/linkbin`, gvproxy v0.9.0 with the patch into `/tmp/src/gvproxy-fd`, the
release binary at `GVPROXY`; then `python3 bench.py restart` and `python3 bench.py speed`.
