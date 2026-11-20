# How much memory an idle computer gives back

To find out.

| | Linux computer | macOS computer |
|---|---|---|
| Memory given back when idle | libkrun returns freed memory on a Mac since 1.19.8; MicroSandbox does not. | To find out. |

Source: the research on the VM engine for Linux computers, 19 November 2026
([research/linux-vm-engine.md](../../../research/linux-vm-engine.md)). How much an idle computer
gives back on a Mac is one of its five measurements, in `linux-vm-engine`.

The taken decision `idle-memory` rests on the answer.
