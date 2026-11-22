# What the first run on macOS shows while it waits

## For the owner

An agent's part, 22 November 2026. You install Silo. Before main exists, macOS has to download more
than 10 GB and then install.

The options:

- Ask during the wait (recommended). The first run asks what to bring in, how main's network starts
  and which rules to switch on while the download runs, and shows how far it is. Main is ready when
  the install ends. It costs answering before main exists, from what Silo found on your Mac.
- Progress and a tour. The first run shows the progress and a short tour of Silo, and asks its
  questions once main is installed. It costs a second wait, after the questions, before main is
  ready.

Recommended: ask during the wait. The questions have to be answered anyway, and answering them while
macOS installs means the wait ends with a main that is ready.

## Notes

How long the first run takes with a macOS main is to find out, in the part `computers`
(`first-run-time-macos`).
