# Agents on the screen of a macOS computer

## The choice

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

## Xcode and simulators need the screen

An agent's note, 24 November 2026. Three things a Mac or iOS developer's agent needs run only with
the computer's user logged in on its screen: the simulator, UI tests, and Xcode's own agent bridge
(`xcrun mcpbridge`, from Xcode 26.3), which needs Xcode open and asks to allow each new agent
process. In a computer, that dialog is on the computer's screen, where an agent or Silo can answer
it; on the developer's own Mac, they click it every time. Mac tests 24 and 29 of `research/2026-11-24-mac-and-ios-developers/README.md` check the
simulator over SSH and the bridge.
