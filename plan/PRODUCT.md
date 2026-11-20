# Silo

What Silo is for, and what holds in every part of it. The parts are listed in
[`parts.json`](parts.json); each has its own page. This page names sentences and decisions by id
and does not repeat them: `plan show <id>` gives each one.

## For

A useful agent needs full access. On your own machine that puts your files, your keys and your
accounts in its reach, and after a long run you cannot tell what it did. Silo gives your agents a
computer of their own, a virtual machine, with a record of what happens in it and a way back
(`the-shape`, `computer`, `record`).

The first user is the owner, on a Mac (`who-it-is-for`). Where computers run after that:
`where-it-runs`. The sentence that says what Silo is, for the home page, is still to decide:
`one-sentence`.

## Why it is so

- **The agent is treated as hostile** (`security-framing`). So inside its computer it has full
  access (`full-access-inside`), and every limit is set outside, at the boundary
  (`where-limits-are-set`, `boundary`).
- **Access is handed down, never up.** Whatever would give a computer more than it has needs
  your yes (`what-an-agent-inside-may-do`).
- **The record is the core of the product** (`the-shape`). Silo tells apart what it saw, what it
  was told and what is only an opinion (`fact`, `report`, `opinion`).
- **What Silo does by itself depends on how sure it is** (`less-sure-less-power`). Only a fact or
  you can grant (`grant-needs-a-fact-or-you`), and a precaution can only protect
  (`precaution-grants-nothing`).
- **Every addition answers the same questions** (`questions-for-an-addition`): what kind each of
  its actions is (`kind-before-design`), what triggers it, and what its bad paths are
  (`bad-paths-first`).
- **How much security work comes now:** `security-is-not-the-focus`.

What Silo is not: `what-silo-is-not`. What it does not build is still to decide:
`what-silo-does-not-build`.

## One example

You want an agent to try a tool you do not trust yet. It works in a fork of main (`fork`,
`main`), with `sudo` and nothing asking for approval (`full-access-inside`). Checkpoints are taken
on their own (`the-shape`). If the try goes wrong, you restore one, or you drop the fork and it
waits in the bin for a time (`restore`, `drop`, `drop-and-bin`).
