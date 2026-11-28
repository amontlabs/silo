# A Linux main on your Mac

## The choice

An agent's part, 28 November 2026. You install Silo on your Mac. Silo downloads an arm64 Ubuntu
image and makes main from it so Claude Code and Codex can work in it with full access and Docker,
while Silo records what happens from outside.

The options:

- Build it in MVP (recommended). Main runs as a Linux computer first, removing the macOS-guest
  unknowns (macOS restore download, Apple provisioning, Apple's two-VM cap, Keychain over SSH) and
  reusing the same Linux computer Silo needs later anyway. A macOS main follows after MVP. It costs
  Xcode and macOS-native builds in main until the macOS main lands.
- Wait for a macOS main first. Keep the Linux main only as a fallback. It costs delaying the first
  working Silo behind unverified macOS-guest work.

Recommended: build it in MVP. polarzero confirmed starting with a Linux main on 28 November 2026.

## Notes

An agent's part, 25 November 2026. It counts 96 open decisions with the first build's other
features, against 87 for the macOS main with the record of model traffic (#373). The 10 that only it
needs, and the 3 whose answer changes, have second opinions in the specs' `decisions.md`.

## polarzero's words, 28 November 2026

Owner: "On os yes we can just do a linux first if that's faster."

An agent's part. So the first option above is the one followed: this feature is in the first
build in place of a macOS main, and `computers-main-on-a-mac` comes after the first build. The
features of the first build that needed a main now need this one.
