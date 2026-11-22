# How a Linux computer's screen is shown

## For the owner

An agent's part, 22 November 2026. An agent drives a browser on the desktop of a Linux fork, and you
want to watch it in a window on your Mac, as you would a macOS computer.

The options:

- The computer's own screen, in a native window (recommended). The virtual machine hands each frame
  of its screen to Silo, and the app draws it. It needs a desktop in the computer and nothing else
  running there, and it uses no network. It costs building: the window, a cursor that moves without
  lag and a resolution that follows the window are Silo's to write, with patches that are not in
  libkrun yet. Sound and GPU drawing are left out at first.
- A remote-desktop server inside the computer. A VNC or RDP server runs in the fork and a viewer
  shows it. It works with tools that exist today. It costs a program inside that an agent with full
  access can stop or change, a port to open through the fork's rules, and a picture that lags.

Recommended: the computer's own screen. A macOS computer is already shown this way, and the two are
to stay consistent. How well it works is to find out by a trial before the Linux desktop is built.

## Notes

To find out. A macOS computer's screen is native.

- A Linux computer needs a desktop in the computer. libkrun hands over each frame; the window is
  Silo's to draw.
- The Mac window, a hardware cursor and resizing are Silo's to build. steamac, an open-source
  project running SteamOS on libkrun on a Mac, carries patches for most of it. Facts as of
  19 November 2026, from [the research in the notes of `linux-vm-engine`](linux-vm-engine.md).
