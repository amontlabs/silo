# How any program on the Mac reaches any computer

The owner said this, on 18 November 2026: people use the app of their agent program on the Mac
and connect it to a machine over SSH. That can be the way into a computer. It is not yet clear
how it works with many computers. Adding each one to SSH by hand is too heavy, a user switches
between computers, and an agent may start other computers. The owner expects this to need
several discussions, and wants it researched first: similar tools, and people who have the same
problem.

The question has three parts: `reaching-a-computer`, `following-the-work` (with
`fork-carries-the-conversation`) and `agents-making-computers`.

## What is known

- Over SSH, the app on the Mac is only the window. The agent program runs inside the computer, so
  its model traffic still crosses the boundary. The Claude desktop app installs Claude Code on
  the remote machine, and the Codex app starts its server there.

## What the research of 18 November 2026 found

- A window finds a conversation in two ways. The user points the app at a machine over SSH (the
  Claude desktop app, the Codex app, T3 Code, Zed, Cursor). Or the conversation announces itself
  through the vendor's account (Claude Code's Remote Control, the vendors' cloud sessions).
- No app documents a way for another program to add a machine to it. Most read the Mac's SSH
  settings. The Codex app lists only the machines named one by one there.
- What people mention most is not adding machines by hand. It is signing in again on every fresh
  machine, several agents in one machine destroying each other's work, and losing sight of what
  sub-agents do.
- The SSH mode of the desktop apps is young. It has open bugs with several conversations on one
  machine, and for Claude it takes no API key. A terminal over SSH is the path that always works.
