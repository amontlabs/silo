# Agents on the screen of a macOS computer

## For the owner

An agent's part, 22 November 2026. An agent in main tests the Mac app it builds by clicking through
it on main's screen. You work in Xcode on your Mac and nothing pops up. When you want to see it, you
open main's screen in Silo.

The options:

- Build it (recommended). Agents use the Mac apps they build or need, and their windows never reach
  your screen. It costs the permissions macOS asks for before a program may control the screen: Silo
  has to grant them inside main with no click from you, which is to find out.
- Leave it out. Agents in main work only on the command line. It costs testing Mac and iOS apps, and
  every agent that works through a screen.

Recommended: build it. It is a reason to give agents a Mac rather than a Linux machine.
