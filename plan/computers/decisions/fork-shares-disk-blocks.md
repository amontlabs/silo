# What does a fork store of its parent's disk

## For the owner

An agent's part, 22 November 2026. Main's disk holds 60 GB, of which 25 GB is build output. You fork
main to try a dependency upgrade.

The options:

- Share the disk (recommended). The fork exists in seconds and takes almost no space. It grows only
  by what it changes, say the 3 GB it rebuilds. It costs simple sizes: a computer's size is two
  numbers, what it holds alone and what it shares, and dropping a computer does not free what its
  forks still share. On a Linux server with the most common Linux file system, the fork reads its
  disk through one more layer, and how much slower that is, is to find out.
- A full copy. The fork is a separate 60 GB on disk and owes nothing to main. It costs 60 GB and
  minutes of copying for every fork, the way VMware and VirtualBox make a full clone.

Recommended: share the disk. Forks are made to try something and then dropped, and 60 GB each would
rule them out on most Macs.
