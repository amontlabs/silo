# Two macOS computers

## The choice

An agent's part, 22 November 2026. Main and a macOS fork where an agent tries a new tool are both
running. You ask Silo to test your changes on both systems. Apple allows two macOS computers running
at once on one Mac.

The options:

- The macOS half waits (recommended). The Linux tests run at once. The macOS tests start when a
  macOS computer stops, and Silo shows that they wait for the fork. It costs time, since the macOS
  result waits for you, and while main and the fork both run it never comes.
- The test is refused. Silo says both places are taken, and you ask again once you stop a computer.
  It costs you coming back, and an agent that asked can only try again later.

Recommended: the macOS half waits. Your ask stays, you see what it waits for, and Silo never stops
your work to make room.

## Notes

The limit is described in `computers`.
