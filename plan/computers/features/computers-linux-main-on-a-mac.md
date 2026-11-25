# A Linux main on your Mac

## The choice

An agent's part, 25 November 2026. You install Silo on a Mac running macOS 26.5. Silo downloads
Ubuntu's 943 MB image and makes main from it in a few minutes. Claude Code and Codex work in it
overnight, with Docker for the tests of the app, and in the morning the record holds the files they
changed, the model calls and their spend, and the processes they left running.

The options:

- Build it in place of a macOS main, for the first build. It costs Xcode and macOS apps in main, and
  the identity of Silo as a Mac of its own for your agents, which polarzero leaned to on 18
  November. A macOS main comes later. It removes the Mac unknowns of a macOS main: macOS 27 on your
  Mac, Apple's provisioning, gvproxy with a macOS computer, the Keychain over SSH.
- Keep it as the fallback (recommended). Build the same Linux computer first as the test computer
  of the boundary and as the start of `computers-linux-beside-main`, and make it main only if the
  Mac's tests of a macOS main fail. It costs nothing that is not needed later anyway.
- Build it later, as `main-on-linux` says today. It costs the fallback.

Recommended: keep it as the fallback, which is why the suggestion is to drop it for now. The specs
are written so that it can be taken the day the Mac's tests say so: [option B's specs](../../../research/2026-11-25-linux-main/README.md), steps 2 to 7, with a test
kit for a Linux host with KVM.

## Notes

An agent's part, 25 November 2026. It counts 96 open decisions with the first build's other
features, against 87 for the macOS main with the record of model traffic (#373). The 10 that only it
needs, and the 3 whose answer changes, have second opinions in the specs' `decisions.md`.
