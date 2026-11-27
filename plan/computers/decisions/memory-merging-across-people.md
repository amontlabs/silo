# Merging identical memory between people

## The choice

An agent's part, 25 November 2026. Five people each run main from the same Linux image on one
host. Much of their memory is the same: the same kernel, the same programs, the same files read.
Linux can merge identical pages into one copy (KSM), and some hosts turn it on to fit more
computers.

Measured ([the research](../../../research/2026-11-25-shared-linux-host/README.md)):

- Five idle Debian 12 computers of 1 GiB each held 1,896 MiB in all. With KSM on, it found 998 to
  1,012 MiB it could free within a minute, none of it pages of zeros. Its scanner used about 20 s
  of processor in the first two minutes.
- KSM merged the pages of two different Unix users. A process of Ben's filled 256 pages with
  guesses of what a page of Ana's held. Writing to the one page that matched Ana's took 10 to
  21 µs, against about 0.1 µs for the other 255. So Ben learned which page Ana held. This is a known
  attack on hosts (CVE-2015-2877; Proxmox advises turning KSM off for hosting).

The options:

- Off across people (recommended). It costs the memory KSM frees, about 200 MiB per idle computer
  here.
- On for the whole host. It costs what the case above shows: one person can test what another's
  computer holds in memory.
- Shared only between one person's own computers. KSM cannot do it: it merges any mergeable
  page of any process, and Linux 6.18 has no setting to hold it to a group. It can be had by
  construction instead, once memory snapshots exist (`forks-of-running-linux`): a person's forks
  map that person's snapshot copy-on-write and share every page they have not written, with
  nobody else's. See the measurement below.

Recommended: off. `idle-computer-gives-memory-back` already returns free memory to the host;
what KSM adds beyond it is shared content, which is the part that leaks.

## Shared per person, measured

An agent's part, 26 November 2026 ([the research](../../../research/2026-11-25-shared-host-leaks/README.md)). With KSM off, Ana's forks
mapped her snapshot copy-on-write and counted its pages once; Ben, probing with a snapshot of his
own, found nothing: every write cost the same, whichever page Ana held. A Debian 12 guest kept
263 MiB of non-zero memory equal to its snapshot after 10 idle minutes, and 117 MiB while busy.
That is about what KSM freed per idle computer (200 MiB), without the leak. It needs memory
snapshots, which a Linux fork does not have yet; until then, off costs that memory. Forks of one
snapshot also share its random state and must reseed when they learn they are copies (Linux's VM
generation ID), within one person.
