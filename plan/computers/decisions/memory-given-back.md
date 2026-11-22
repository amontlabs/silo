# How much memory an idle computer gives back

## For the owner

An agent's part, 22 November 2026. A build in main takes 6 GB of memory, then main sits idle all
night. A macOS computer beside it does the same after an Xcode build. How much of that memory each
one gives back to your Mac decides how many computers fit.

The options:

- Measured at each release, against a target set first (recommended). One benchmark runs a build,
  waits five minutes and reads what each kind of computer still holds, on a Mac and on a Linux host.
  The numbers are published, and a kind of computer that misses its target or gives nothing back
  comes back to you. It costs a run of the benchmark at each release. Until its first run nobody
  knows whether a macOS computer gives anything back, so `idle-memory`, which you took, is not shown
  to hold there.
- Measured once, before the first release. It costs what changes later: an update of macOS, or of
  what runs Linux computers, can stop memory coming back, and nothing would show it.

Recommended: measured at each release. A Mac holds a few computers, not fifty, and memory given back
is what lets them share it, so the number has to stay true.

## Notes

To find out.

| | Linux computer | macOS computer |
|---|---|---|
| Memory given back when idle | libkrun returns freed memory on a Mac since 1.19.8; MicroSandbox does not. | To find out. |

Source: the research on the VM engine for Linux computers, 19 November 2026
([the research in the notes of `linux-vm-engine`](../../engine/decisions/linux-vm-engine.md)). How much an idle computer
gives back on a Mac is one of its five measurements, in `linux-vm-engine`.

The taken decision `idle-memory` rests on the answer.
