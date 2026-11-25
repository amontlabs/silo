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
- On only between one person's own computers. KSM has no setting for that that we found: it merges
  any mergeable pages of any process.

Recommended: off. `idle-computer-gives-memory-back` already returns free memory to the host;
what KSM adds beyond it is shared content, which is the part that leaks.
