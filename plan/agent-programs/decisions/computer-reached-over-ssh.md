# Which way in reaches a computer

## The choice

An agent's part, 22 November 2026. A test fails in main and you want to fix it yourself. You open
main's `~/work/app` in your editor on your Mac, with a terminal beside it. You also want to talk to
Claude Code in main from the Claude desktop app, which connects to a machine over SSH.

The options:

- SSH, with your own tools (recommended). `ssh main` opens a terminal, your editor or T3 Code opens
  main's folder, the agent apps on your Mac reach main as they reach any server, and main's screen
  opens in Silo. Nothing of your Mac is passed in, your SSH keys included. It costs a server running
  inside each computer, which an agent with full access can stop. You then restore a checkpoint to
  get it back. It also costs one risk you take on with some editors: VS Code and editors built on it
  run a server inside main that stays linked to your Mac, and VS Code's own documentation says a
  compromised machine can use that link to run code on your Mac. Silo shows that link while it is
  open.
- Only Silo's own way in: a `silo shell main` command, or a terminal and an editor inside Silo's
  app. Nothing runs inside the computer for it, and nothing of main reaches back to your Mac. It
  costs every tool that connects over SSH: your editor and the agent apps cannot open main. You work
  in Silo's editor instead of your own, and Silo has to build that editor.

Recommended: SSH, with your own tools. Every agent app that works on another machine today does it
over SSH, people will not give up their editor, and reaching every computer by name rests on it. A
terminal over SSH has no link back, so an editor's link is a choice you make knowing what it allows.
