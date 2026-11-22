# Registering an extension

## For the owner

An agent's part, 22 November 2026. You installed an extension that sums up main's day each
evening and gives its opinion when a request waits for it. Its author changes one of its files, and
one evening its summary is still running when its next wake comes.

The options:

- Strict (recommended). A changed file is a new version: nothing of it runs until you import and
  install it again, and you see what changed. A wake that comes while the same wake still runs is
  skipped and recorded. A request that waits always gets its own answer, up to a fixed number at
  once, and past that it ends as the extension says it should. Each command runs for a limited
  time. It costs a click for every update, and a slow extension loses wakes.
- Lenient. Files update in place, and every wake runs however many pile up. It costs control: code
  you never accepted runs with what you granted, and a slow extension can pile up commands until
  your Mac slows down.

Recommended: strict. What you granted was granted to the version you saw, and a pile of an
extension's commands should never be what slows main.

## Notes

Not designed. What wakes an extension is put forward in `what-wakes-an-extension`; how it is installed is the
later feature `installing-an-extension`.
