# How do you work in a computer yourself

## For the owner

An agent's part, 22 November 2026. A test fails in main and you want to fix it yourself. You open
main's `~/work/app` in your editor on your Mac, with a terminal beside it.

The options:

- Your own tools over SSH (recommended). `ssh main` opens a terminal, your editor or T3 Code opens
  main's folder, and main's screen opens in Silo. Nothing of your Mac is passed in, your SSH keys
  included. It costs one risk you take on with some editors: VS Code and editors built on it run a
  server inside main that stays linked to your Mac, and VS Code's own documentation says a
  compromised machine can use that link to run code on your Mac. Silo shows that link while it is
  open.
- A terminal and an editor inside Silo's app. Nothing of main reaches back to your Mac. It costs
  your setup, since you work in Silo's editor instead of your own, and the work of building that
  editor.

Recommended: your own tools over SSH. People will not give up their editor, and a terminal over SSH
has no link back; an editor's link is then a choice you make knowing what it allows.
