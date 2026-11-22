# One piece for every agent program

## For the owner

An agent's part, 22 November 2026. Your recipe includes two pieces, one for Rust and one for code
review, and main runs Claude Code and Codex. Each piece's instructions are written once.

The options:

- One instructions file per program, the pieces in order (recommended). Both pieces' instructions go
  into ~/.claude/CLAUDE.md for Claude Code and into ~/.codex/AGENTS.md for Codex, one after the
  other, each under a heading that names its piece. Skills go into each program's folder of skills,
  and two pieces with a skill of the same name refuse the recipe, naming both. A program with no
  known place for instructions gets nothing, and Silo says so. It costs tokens: every line of every
  piece is read at the start of every conversation in the computer.
- Instructions as skills only. Each piece's text becomes a skill, which a program reads only when
  the work calls for it. It costs the rules that must always hold, such as "never push to main": an
  agent may never read them.

Recommended: one instructions file per program. A piece's instructions are meant to be followed
every time, and that is what this file is for in each program.

## Notes

The first two sentences are the owner's, 18 November 2026. The third is a proposal. The data file
of an agent program, which says its name, where its sessions are and how it takes a
conversation up again, belongs to `agent-programs`.
