# The approvals of computer use

## The choice

An agent's part, 22 November 2026. An agent needs to drive the screen of a macOS computer. macOS
allows that only after two approvals in System Settings: Accessibility and Screen Recording. The old
Silo set them itself while it made the computer, with System Integrity Protection turned off inside
it.

The options:

- Try the old way on the current macOS (recommended). While the computer is made, protection is off
  and Silo writes the two approvals. The trial answers whether they work, whether protection can
  then be turned back on with the approvals kept, whether a fork keeps them, and whether macOS asks
  for Screen Recording again later, as macOS 15 began to do. It costs a way Apple does not offer,
  which a macOS update can break. If protection cannot be turned back on, a computer would be handed
  over with it off, and that comes back to you first.
- Approvals by hand. You give the two approvals once in each macOS computer made from a recipe, and
  its forks keep them. It costs two clicks for each new computer, and more whenever macOS asks for
  Screen Recording again.

Recommended: try the old way. Computer use that asks nothing is what lets agents work on a macOS
computer on their own, and the trial shows what it costs before anything ships.

## Notes

To find out. It decides whether `no-approval-by-hand` can hold on macOS.

What is known: the old Silo did this on macOS computers. During set-up it wrote the approvals for
Accessibility and Screen Recording into the system's permission database, with System Integrity
Protection turned off in the computer. A fork's disk copy carried them.

Also to find out: whether it holds for a fork of a running computer.

The old Silo's set-up script says it follows what GitHub's images for macOS test machines do.
That was not checked against those images, and it says nothing about the current macOS.
