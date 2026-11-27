# Can an agent drive the screen of a macOS computer

A desktop in Linux computers is a later feature: `linux-desktop`.

## How, on macOS 27, 25 November 2026

An agent's part, from #352 (files 10 and 13). This answer does not say how, and the obvious way does
not work: on macOS 27 a program started over SSH cannot be given Accessibility or Screen Recording
from inside without turning SIP off, and the database that grants them is an undocumented detail.
Driving the screen from outside, through the virtualization framework's display and input, needs no
permission inside the computer. The answer stays as it is; whoever writes `computers-macos-screen`
starts there.
