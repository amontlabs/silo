# Silo

What Silo is for, and what holds in every part of it. The parts are listed in
[`parts.json`](parts.json); each has its own page. This page names statements and decisions by id
and does not repeat them: `plan show <id>` gives each one.

## For

Silo gives your agents a computer of their own and stays outside it, where they cannot reach
(`one-position`, still to decide). From there it keeps a record of what happens in the computer,
holds what the agent says against what it saw, can make a request wait at the boundary, and keeps
a way back (`the-shape`, `record`, `boundary`).

What Silo cannot see from there is said too: `what-silo-cannot-see`, still to decide.

The first user is polarzero, on a Mac (`who-it-is-for`). Where computers run after that:
`where-it-runs`. The sentence that says what Silo is, for the home page, is still to decide:
`one-sentence`.

## Why it is so

- **The agent is treated as hostile** (`security-framing`). Inside its computer:
  `full-access-inside`. Where limits are set: `where-limits-are-set`, `boundary`.
- Whatever would give a computer more than it has: `what-an-agent-inside-may-do`.
- **The record is the core of the product** (`the-shape`). Silo tells apart what it saw, what it
  was told and what is only an opinion (`fact`, `report`, `opinion`).
- **What Silo does by itself depends on how sure it is** (`less-sure-less-power`). Only a fact or
  you can grant (`grant-needs-a-fact-or-you`), and a precaution can only protect
  (`precaution-grants-nothing`).
- **The core holds the mechanism, and a use of it is an extension** (`mechanism-or-use`, still to
  decide). Some extensions ship with Silo (`what-ships-with-silo`,
  `spend-and-budgets-ship-as-an-extension`, `first-extensions-that-ship`).
- **Each thing is done in the most reliable way that can do it**, a rule wherever a rule can do it
  (`which-way-first`, still to decide).
- **Every addition answers the same questions** (`questions-for-an-addition`): what kind each of
  its actions is (`kind-before-design`), what triggers it, and what its bad paths are
  (`bad-paths-principle`, still to decide).
- **How much security work comes now:** `security-is-not-the-focus`.

What Silo is not: `what-silo-is-not`. What it does not build is still to decide:
`what-silo-does-not-build`.

## Two examples

You want an agent to try a tool you do not trust yet. It works in a fork of main (`git-words`,
`main`), with `sudo` and nothing asking for approval (`full-access-inside`). Checkpoints are taken
on their own (`the-shape`). If the try goes wrong, you restore one, or you drop the fork and it
waits in the bin (`git-words`, `drop-and-bin`).

You let an agent run with every approval off. It ends its turn with "All tests pass." Silo read
the model calls at the boundary and saw the files change from outside (`fact`). The agent's
sentence is a `report`. An extension that ships with Silo holds one against the other and gives
its `opinion`: a test file was deleted before the last run (`said-against-done`, still to decide).
