# One Linux host for a team of five

Research of 25 November 2026, by an agent. #362 found that a host several people of a team share
has to be Linux, since macOS's licence lets one user use a Mac at a time (`shared-host-system`).
#366 found what a Linux host needs for one person. This asks what changes when five people share
it: keeping their computers apart, their secrets, their records and budgets, and the host's size.
Each answer is a new open entry of the plan, listed at the end.

## In short

1. **The kernel keeps people apart, not the engine.** Each person is a Unix user of the host, with
   a control group of their own. Without one, the host's out-of-memory stopped another person's
   main; with one, it stopped a computer of the person who ran out.
2. **A computer must be known by its link, never by its address.** On a shared bridge, a computer
   that took the address of a stopped computer got that computer's owner's cloud token from a
   metadata server that tells computers apart by address. A link per computer gave its own token
   whatever address it claimed.
3. **Memory merging (KSM) is off between people.** It freed about half the memory of five idle
   computers, and it let one person tell which page of memory another person held.
4. **A disk share must count shared blocks once.** Counted file by file, four checkpoints of a
   3 GiB disk count 15 GiB for 3.4 GiB real. Btrfs quota groups count them right; the engine can
   too, from #365's map of changed blocks.
5. **The host's administrator can read every person's secrets and records.** Another person
   cannot. Silo should say so plainly rather than promise otherwise.
6. **A pooled budget is exact on one host**, which #362 found it is not across Macs.

## Where it was measured

A Linux cloud container: kernel 6.18, 4 processors, 15 GiB of memory, cgroup v1, no `/dev/kvm`.
People are Unix users (`silo-a`, `silo-b`); computers are network namespaces or processes standing
in for them, except the five guests of finding 3, booted with QEMU without KVM (TCG). The scripts
are throwaway code, run as root; each cleans up after itself.

| Script | What it measures | Output |
|---|---|---|
| `oom.sh`, `hog.py` | Whose computer stops when the host runs out of memory | `results-oom.txt` |
| `net.sh`, `meta.py` | How a shared metadata server tells computers apart, and what a computer reaches | `results-net.txt` |
| `creds.sh`, `holder.c`, `reader.py` | Who can read a secret in a person's boundary process, or use their SSH agent | `results-creds.txt` |
| `ksm.sh`, `ksm.c` | Whether KSM merges two people's pages, and what one learns of the other's | `results-ksm.txt` |
| `ksm-guests.sh`, `guestram.py` | What KSM frees on five idle Debian 12 guests | `results-ksm-guests*.txt` |
| `quota.sh` | One person filling the disk, with and without a per-person limit | `results-quota-*.txt` |

## 1. Memory: a control group per person

A parent control group of 8 GiB plays the host. Ben runs one computer of 4 GiB. Ana starts three
that grow to 1.5 GiB each, so the host runs out because of Ana.

| | Ben's computer | Ana's three computers |
|---|---|---|
| Everyone in the host's group only | **stopped** | running |
| Ana's computers also in a group of hers, 3.5 GiB | running | one stopped |

The kernel stops the largest process in the group that ran out. Without a group per person, that
is whoever has the largest computer.

## 2. The network: a computer known by its link

Two computers, Ana's and Ben's. A metadata server on the host plays the boundary's answer at
169.254.169.254 (`cloud-credentials`, measured by #379), and answers with a token for the
computer's owner. An agent in a computer is root there and sets any address.

| Layout | Ben asks | Ana's computer stopped, Ben takes its address | Ben reaches Ana's computer | Ben reaches a host service on every address |
|---|---|---|---|---|
| One bridge, server knows the address | token of Ben | **token of Ana** | yes | yes |
| One bridge, ports isolated, each held to its address and MAC by `nft` | token of Ben | no answer | no | yes |
| A link per computer, the server bound to each link | token of Ben | token of Ben | no | yes |

- A bridge is what libvirt and most Firecracker setups use. Silo's network stack per computer
  (gvproxy today, Silo's own later) is the third row already.
- No layout keeps a computer from a service on the host that listens on every address. The host
  needs a firewall rule per link that lets through only the boundary's ports. Not measured with it.

## 3. Secrets: another person cannot read them, the administrator can

A process standing in for Ana's boundary holds a token in its memory. Others read
`/proc/<pid>/mem`.

| Reader | As is | Marked not dumpable (`PR_SET_DUMPABLE 0`) |
|---|---|---|
| Another process of Ana's user | read | refused |
| A process of Ben's user | refused | refused |
| root | read | read |

Ana's `ssh-agent`, its socket in a folder of hers with mode 700: Ben was refused, root listed her
key. This kernel has no Yama module; Ubuntu's default (`ptrace_scope` 1) also refuses Ana's other
processes.

Keeping values off the host entirely is not possible while the host sends the requests: a token
swapped in is in the request the host's boundary writes. Only an SSH key can stay on the person's
Mac, which signs for the host; that stops when the Mac sleeps.

## 4. Memory merging: off between people

Two processes of different users, each marking 256 pages mergeable. Ana's hold one "secret" page;
Ben's are 256 guesses, one of which matches. After KSM scans, Ben times one write per page:

| Ana's secret page | Ben's slowest write | The other 255 |
|---|---|---|
| 173 | page 173, 10,143 ns | 105 ns on average |
| 42 | page 42, 20,896 ns | 110 ns on average |
| none (control) | page 139, 695 ns | 114 ns on average |

A write to a merged page takes a copy-on-write fault, so Ben finds the page Ana holds. This is a
known attack (CVE-2015-2877); Proxmox advises turning KSM off for hosting.

What KSM frees: five Debian 12 guests (1 GiB, 1 processor each, the same image) held 1,896 MiB of
guest memory at the login prompt. With KSM on, it found 998 to 1,012 MiB it could free within
60 s, and nothing more in the next 4 minutes. With pages of zeros counted apart, none of it was
zeros: it is shared content, the part that leaks. ksmd used about 20 s of processor over the
first two minutes on a 4-processor machine (QEMU without KVM, so the processor was busy anyway).

## 5. Disk: a share per person, counted once

A 16 GiB Btrfs file system. Ana's main disk of 3 GiB, four checkpoints with 100 MiB written after
each, then Ana writes until refused, then Ben writes 1 GiB.

| Limit | Ana's space as counted after 4 checkpoints | Ana refused at | Ben's 1 GiB |
|---|---|---|---|
| None | 15 GiB counted file by file (3.4 GiB real) | disk full, 15.5 GiB | **refused, disk full** |
| A Btrfs quota group of 6 GiB for Ana's subvolume and its snapshots | 3.4 GiB | her 6 GiB | written |

XFS's quotas are not built into this kernel, so how they count blocks shared by reflink was not
measured. The engine can count each person's real space itself from #365's map of changed blocks,
which works on XFS, the file system #366 prefers for a host.

## 6. Spend and size

Not measured here; reasoned from #362, #365 and #374, and written as entries:

- One budget for the host is exact: every boundary on it reserves against one count the engine
  keeps, as #374's prototype does for one computer.
- Five people at `sizing-target`'s common load (a 16 GB Mac each) need 80 GB. An idle Debian 12
  guest held about 380 MiB here; one Claude Code process held 295 MiB during a long session. With
  #365's 250 to 410 GiB of checkpoints per main over 30 days, five mains take 1.25 to 2 TiB. A
  server of 128 GB, 16 cores and 4 TB of NVMe with KVM is proposed.

## What it changes in the plan

No verdict is written and no taken entry is changed. New open entries, each with its case, its
options and a recommendation, in the stage "later" after `teams`:

| Entry | Recommended |
|---|---|
| `members-on-a-host` | A Unix user and a control group with a memory limit per person |
| `computer-known-by-its-link` | A link per computer; an address never names a computer |
| `host-out-of-reach` | A computer reaches only its boundary on the host |
| `memory-merging-across-people` | KSM off |
| `disk-share-on-a-host` | A share per person, counted by the engine, shared blocks once |
| `secrets-on-a-shared-host` | In the person's own boundary process; the administrator can read them, and Silo says so |
| `records-on-a-shared-host` | Read by its owner and whom they share it with |
| `pooled-budget-on-a-host` | A pooled limit beside each person's budget |
| `team-host-size` | 128 GB, 16 cores, 4 TB NVMe, KVM |

Notes are added to `linux-server-host` and `teams`, listing these entries.

With #362, if both are merged: `members-on-a-host` assumes `shared-host-system`'s answer (a Linux
server), and `person-in-a-control-fact` already names who did what on such a host. The entries
here need only entries on `main`, so either PR merges first.

## Not verified

- No KVM: no real virtual machine monitor. Computers are network namespaces, processes, or QEMU
  guests without KVM, whose memory behaviour is the guest's but whose speed is not.
- Builds, tests and language servers inside main, which are most of a computer's memory; processor
  use of five people at once.
- XFS quotas and shared blocks; Btrfs quota groups' cost with many snapshots.
- The host firewall rule of `host-out-of-reach`, and Yama's `ptrace_scope`.
- Whether any Linux release can limit KSM to one person's processes. None we read does.
- cgroup v2, which a real host uses; the same limits exist there (`memory.max`), not run here.
