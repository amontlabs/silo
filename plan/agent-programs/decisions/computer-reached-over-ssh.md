# Which way in reaches a computer

## For the owner

An agent's part, 22 November 2026. You want to open main in your editor, and to talk to Claude Code
in main from the Claude desktop app, which connects to a machine over SSH.

The options:

- SSH (recommended). Your terminal, your editor and the agent apps on your Mac reach main as they
  reach any server. It costs a server running inside each computer, which an agent with full access
  can stop. You then restore a checkpoint to get it back.
- Only Silo's own way in, such as a `silo shell main` command. Nothing runs inside the computer for
  it. It costs every tool that connects over SSH: your editor and the agent apps cannot open main.

Recommended: SSH. Every agent app that works on another machine today does it over SSH, and reaching
every computer by name rests on it.
