# Which Mac Silo needs

## The choice

An agent's part, 24 November 2026. You install Silo on a Mac that runs macOS 26.5, as polarzero's
did on 21 November. After Apple's installer has put macOS in main, macOS starts on Setup
Assistant: it asks for a region, a user and a password, and SSH is off. Nobody is there to click.

The options:

- macOS 27 or later (recommended). macOS 27 adds `VZMacGuestProvisioningOptions`: Silo passes a
  full name, a user name, a password, automatic login and "turn on Remote Login" in the options of
  the computer's first start, and Setup Assistant creates the user and turns SSH on by itself. It
  costs every Mac on macOS 26: the first run asks you to update first. It needs macOS 27 on the
  Mac and in the computer, and Apple honours it only on a computer that is not set up yet.
- macOS 26 too, with keystrokes. Silo types through Setup Assistant over the computer's screen, as
  Tart's Packer templates do. It costs a script timed screen by screen, which breaks when Apple
  changes a screen: Tart's needed eight fixes in 14 months.
- macOS 26 too, by writing the disk before the first start. Silo writes the user, the flag that
  Setup Assistant is done and the SSH service onto the new disk from outside, as Lima does for
  Linux. Nothing inside has run yet, so that disk is not hostile. It costs reading and writing
  Apple's private formats, which no document describes.

Recommended: macOS 27 or later. It is Apple's own way, and the first user's Mac can update.

## Notes

Agent, 24 November 2026. From the research of that day (drafts #352, file 10, and #354), which read
Apple's session 224 of WWDC 2026, Tart's templates and the developer forums. Nothing has run on a
Mac yet.

- A developer reported that provisioning fails when the computer's user name equals the Mac's login
  name (FB23241619, no fix reported). `agent-programs-moving-in` wants the same name, so the first
  Mac test provisions once with each.
- Apple's framework is not safe in a daemon, so the program that holds a computer runs in your login
  session, and after a restart of the Mac main comes back once you log in.
- `first-run-failure` already checks the system version before it changes anything; this entry says
  which version it checks for.
