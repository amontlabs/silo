---
"silo-ui": patch
---

A macOS computer that an agent may have used no longer starts on Apple's own network. Forks, imports and any computer whose setup has finished come up behind Silo's network gateway, so the network rules apply during their setup too. Silo prepares the stopped disk first. If a disk lacks Silo's network setup (a copy of an old checkpoint, or an export from before the gateway existed), Silo stops with a message that says to make the computer again from one Silo has set up. The computer you start in Recovery, to turn off System Integrity Protection, has no network at all. A first install on a fresh disk is unchanged.
