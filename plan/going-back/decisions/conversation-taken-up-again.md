# Whether a conversation is taken up again after a restore or a fork

## The choice

An agent's part, 22 November 2026. You restore main to 15:38. Claude Code was halfway through a
refactor then, and its conversation is a file on main's disk, as it was at 15:38.

The options:

- Said per program, after a check (recommended). Silo checks each agent program it supports and says
  which ones take a conversation up again after a restore and in a fork. With Claude Code you run
  `claude --resume` and carry on from 15:38. It costs a check for each program and each of its
  updates, and a program not checked is shown as not checked.
- No word from Silo. The file is there, and whether the program can use it is the program's
  business. It costs a surprise after the restore, when an agent turns out unable to carry on.

Recommended: said per program, after a check. Going back with the conversation is worth something
only if the agent can carry on from it, so Silo should know which agents can before you rely on it.

## Notes

This line comes from the plan's notes. Whether each agent program does so in a copied computer is
to find out: the open decision `conversation-resumes-in-a-copy` in `agent-programs`.
