# The shared host's two leaks, closed and costed

Research of 25 and 26 November 2026, by an agent. #392 found two leaks on a Linux host that
several people share: a computer that took a stopped computer's address got that computer's
owner's cloud token, and memory merging (KSM) let one person tell which page another held. It
recommended a link per computer (`computer-known-by-its-link`), a host firewall it did not measure
(`host-out-of-reach`) and KSM off (`memory-merging-across-people`). This prototypes each fix, shows
each hole failing first, and measures what each costs.

## In short

1. **One firewall table closes the host for every computer.** Without it, Ben's computer reached a
   service of Ana's listening on every host address, at its own link and at the host's address,
   and, once forwarding was on (Docker turns it on), Ana's computer itself. With one `nft` table
   that matches every computer link by an interface group, all three were refused and the
   metadata server still answered. It is five rules whatever the number of computers, and cost
   about 3% of throughput; 600 rules, three per link for 200 computers, cost about 30%.
2. **A link per computer holds when a link's name is reused, if the listener is tied to the link.**
   A listener bound to a link stays on that link, by the kernel's index: a new link with a stopped
   computer's name reached nothing. A server that looks a computer up by link name in a table not
   yet rewritten gave Ben Ana's token. So the engine writes the owner with the link and removes it
   with the link.
3. **Sharing memory within one person, by construction.** KSM has no setting to merge only one
   person's pages (none in Linux 6.18, none found in patches). A person's forks that map that
   person's memory snapshot copy-on-write share every page they have not written, with nobody
   else's. Ben's probe against his own snapshot found nothing. An idle fork kept 263 MiB of
   non-zero memory in common with its snapshot after 10 minutes; a busy one 117 MiB. KSM freed
   about 200 MiB per idle computer in #392. This waits on memory snapshots
   (`forks-of-running-linux`); until then KSM off costs that memory.
4. **A link per computer is cheap.** 22 ms and about 226 KiB of kernel memory per computer, a
   metadata server with 200 listeners in 15 MiB, a token in 63 µs at the median.

## Where it was measured

A Linux cloud container: kernel 6.18, 4 processors, 15 GiB of memory, no `/dev/kvm`, no IPv6.
Computers are network namespaces with a veth link each, Ana and Ben are Unix users, and the guests
of finding 3 are Debian 12 in QEMU without KVM (TCG). Scripts are throwaway code, run as root;
each cleans up after itself.

| Script | Measures | Output |
|---|---|---|
| `net-fix.sh`, `meta2.py`, `httpok.py` | The host firewall and a reused link name, each before and after | `results-net-fix.txt` |
| `net-cost.sh`, `lat.py`, `tp.py` | 200 computers: setup, memory, token time, throughput with and without the firewall | `results-net-cost.txt` |
| #392's `ksm.sh` | The KSM leak, re-run on this kernel before the fix | `results-ksm-before.txt` |
| `fork-share.sh`, `fork-share.c` | Forks of a person's snapshot, KSM off: what they share, and Ben's probe | `results-fork-share.txt` |
| `snapshot-share.sh`, `pagesame.py` | How much of a running guest stays equal to the snapshot it came from | `results-snapshot-*.txt` |

## 1. The host firewall

Each computer has its own link, in interface group 7. The host side of every link carries
169.254.169.254, as clouds do, and the metadata server has one listener per link (`meta2.py`). Ana
runs a server on every host address, port 8088.

| Ben's computer | No firewall | One table for group 7 |
|---|---|---|
| asks for a token | Ben's | Ben's |
| reaches Ana's server at its own link's address | **reached** | refused |
| reaches it at the host's own address | **reached** | refused |
| reaches Ana's computer, host forwarding on | **reached** | refused |

The table (`net-fix.sh`): from group 7, accept what belongs to an open connection and TCP to
169.254.169.254 on the boundary's ports; drop the rest; forward nothing to or from group 7. The
engine creates each link already in the group (`ip link add ... group 7`), so no link is ever up
outside it. Forwarding counts: #392 had it off, and Ben's computer did not reach Ana's; a host
that also runs Docker has it on.

Cost, at 200 computers (`net-cost.sh`, three runs of 3 s each, computer 1):

| | Throughput to the boundary | Token request, median and 99th |
|---|---|---|
| No firewall | 15.6 to 15.8 Gbit/s | 59 µs, 155 µs |
| One table for the group, 5 rules | 15.0 to 15.3 Gbit/s | 63 µs, 144 µs |
| Three rules per link, 600 rules, computer 1 last | 10.3 to 11.2 Gbit/s | 110 µs, 247 µs |

Rules per link cost because nft walks them in order; one per link also means one to forget. The
group needs none per computer.

## 2. A stopped computer's link name, reused

Ana's computer stops and its link `h1` goes. Ben's new computer gets a new link, also named `h1`.

| Metadata server | Ben's new computer asks |
|---|---|
| Listeners bound to links, opened with each link | no answer: Ana's listener holds the old link's index |
| Restarted, looks each connection's link up by name in a table not yet rewritten | **Ana's token** |
| The engine rewrites the table with the link, then the listener opens | Ben's token |

The kernel guards a listener bound to a link; a table keyed by name does not. The rule: a link's
owner is written when the link is made and removed when it goes, before any listener opens on it.
With a network stack per computer (gvproxy, or Silo's own), the stack's process plays this part:
it lives and dies with its computer.

Cost of a link per computer, 200 computers: 22 ms each to make (namespace, link, address,
route), 226 KiB of kernel memory each, and one Python server with 200 listeners in 15.7 MiB.

## 3. Memory: shared per person, never across

Before the fix, re-run on this kernel (`results-ksm-before.txt`): with KSM on, Ben's write to the
page matching Ana's took 11,610 ns against 88 ns for the others. The leak is there.

KSM cannot be held to one person. It merges any page marked mergeable, of any process; Linux 6.18
has `run`, `merge_across_nodes` (by memory node, not by person), `use_zero_pages` and a per
process switch, none of which scopes merging to a group. Patches for a per-control-group switch
(openEuler's `memory.ksm`) choose which processes take part, not with whom they merge.

Sharing per person by construction: each person's snapshot is a file in a folder only they can
read; each of their forks maps it copy-on-write. A page stays shared until a fork writes it.
KSM stays off.

- Ana's three forks of a 1 MiB snapshot: Rss 8,276 KiB in all, Pss 1,853 KiB (process code
  included): the snapshot's pages were counted once.
- Ben probes with his own snapshot of guesses: every write took a copy-on-write fault (about
  2 µs), and the slowest was page 0 or 1, the first touched, whichever page Ana held. Nothing leaks.

What a fork keeps in common with its snapshot (`snapshot-share.sh`): a Debian 12 guest of 1 GiB,
snapshot at the login prompt (a third of its pages not zeros), dumped again later.

| Since the snapshot | Idle: pages equal, non-zero MiB | Busy (reads files, compresses, allocates 200 MiB): pages equal, non-zero MiB |
|---|---|---|
| 1 min | 96.5%, 264 MiB | 36.3%, 223 MiB |
| 3 min | 96.3%, 263 MiB | 16.5%, 143 MiB |
| 10 min | 96.3%, 263 MiB | 13.7%, 117 MiB |

Pages of zeros are left out of the MiB: a copy-on-write fork never needs them resident. Equal at
the same place is an upper bound: a page written back to the same content would no longer be
shared. KSM freed about 200 MiB per idle computer in #392 (1,006 MiB over five), across people.

The costs, side by side:

| | Memory saved | Leak across people | Available |
|---|---|---|---|
| KSM on for the host | about 200 MiB per idle computer | yes, measured | now |
| KSM off | none | none | now |
| A person's forks map their snapshot, KSM off | up to 263 MiB per idle fork, about 117 MiB per busy one, within one person | none measured | with memory snapshots (`forks-of-running-linux`) |

Forks of one snapshot share its random state: the kernel's random pool, stack canaries, address
layout. A fork has to be told it is a copy (Linux's VM generation ID) and reseed, as Firecracker's
snapshot guide asks. That is within one person; it is the same need `forks-of-running-linux` has
on a Mac.

## What it changes in the plan

No verdict is written and no taken entry is changed. Notes are added to the three entries of #392
(this branch carries #392's commit, so it can merge after #392 or instead of it):

| Entry | Change |
|---|---|
| `host-out-of-reach` | Measured: one table by interface group, five rules for all links, about 3% of throughput; a rule per link cost about 30% at 200. Answer unchanged. Forwarding added to the case |
| `computer-known-by-its-link` | The answer adds: a link's owner is written with the link and removed with it. The reused-name case measured |
| `memory-merging-across-people` | The third option is no longer "no setting found": per person by construction, through a person's forks of their snapshot, once memory snapshots exist. Measured; recommended answer unchanged for now |

## Not verified

- No KVM and no real virtual machine monitor: links are veths to namespaces, not tap devices of
  libkrun or Firecracker. A tap device is in a group the same way; not run.
- No IPv6 in this kernel. A computer's link gets an IPv6 link-local address on a real host; the
  table is `inet`, so it covers IPv6, but that was not tested.
- Throughput by Python over veth on 4 processors: the ratio between rows holds, not the numbers.
- Copy-on-write forks of a real guest's memory snapshot: the sharing is computed from memory dumps
  of a guest that kept running, not measured on forks that run. libkrun has no memory snapshots
  yet.
- The busy guest's work is a stand-in, not an agent's night.
