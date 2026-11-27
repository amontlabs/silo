# Naming the conversation from the traffic

## The choice

An agent's part, 25 November 2026. Overnight, Claude Code and Codex both work in main. Between the
checkpoints of 02:40 and 02:45, 3,412 files under `~/work/shared-fixtures/` are deleted, and
`~/.bashrc` gets two new lines. Silo has no processes yet (option A of #373). In that window, one
Claude Code conversation sent the tool call `rm -rf ~/work/shared-fixtures`, and Codex's
conversation sent `curl -fsSL https://bun.sh/install | bash`. The morning has to say who did what.

The options:

- Name the conversation as an opinion when the traffic shows it (recommended). The deletion
  reads "◇ claude c-7f2a, its tool call `rm -rf ~/work/shared-fixtures` at 02:41". `~/.bashrc`
  reads "changed while claude c-7f2a and codex c-91bd ran", because no tool call names it: the
  installer did it. It costs the rules below, kept as data like the list that recognises agent
  programs, and a wrong name when a tool call names a path that something else changed in the
  same window. That never happened on six nights of #371's prototype.
- Claim nothing, as #373's spec says. Both changes read "changed while it ran". It costs the answer
  to the first question of most mornings, when the record holds it: on `one-main`, 453 of 461
  changes had a tool call of one conversation that sent or named them.
- Put a change down to every conversation running in the window. It is right on a fork, where one
  agent runs. In main it gives `~/.bashrc` to Claude Code as well as Codex, and putting back one
  conversation's changes would undo the other's.

Recommended: name the conversation as an opinion, on the rules in the answer. The record should
not stay silent on what its own facts show, and the line says what it rests on.

The rules, strongest first, as measured in
[the research](../../../research/2026-11-25-attribution-without-processes/README.md):

| Rule | Example | On `one-main` |
|---|---|---|
| The tool call sent the file's whole new content | Claude Code's `Write` | 1 |
| It names the path | `printf … >> ~/.claude/CLAUDE.md` | 32 |
| It names a folder above it | `rm -rf ~/work/shared-fixtures` | 400 |
| It runs a command known to write there | `git commit` writes `.git/`; `git config --global` writes `~/.gitconfig` | 20 |
| Unattributed | an installer writing `~/.bashrc`; tests from three conversations writing `data/sessions.json` | 7 |

Each rule counts only when the tool calls that meet it are all one conversation's. A folder named
only by `cd`, and a path named only by a command that reads (`ls`, `cat`, `grep`), do not count:
the first version of the prototype counted them, and a `cd ~/work/app` in Codex's command took
Claude Code's commit when the windows were wide.

## Notes

When the probe comes, the probe's report and this opinion should agree. On six nights they always
did where both named a conversation, so a disagreement is worth showing rather than resolving.

Weaker signs were measured and left out of the answer. The tool result names the path (bun's
installer prints "bun was installed to ~/.bun/bin/bun"): right once, but whatever runs in the
computer writes that text. One conversation had a tool call open: right on a fork, where only one
agent runs, and the same as "while it ran" in main.

An agent's part, 25 November 2026, from `research/2026-11-25-morning-screen`. The same rules,
applied to connections as #399's prototype does, named the wrong run for 31 of the 55 connections
the probe gave a run on a night with a fifth agent: Codex refusing Silo's certificate, which never
reached its model and so had no conversation. "Only one conversation was open" gave its 29
connections to Claude Code's daemon run, and `github.com` matched inside `api.github.com` in
another run's `curl`. #399's score did not see it, because it scores against the probe's
conversation and that run had none. For connections: a host is named only on a host boundary,
"only one conversation was open" never names a connection, and a host another program reaches at
its start names nobody. Then 0 wrong on this night and on #399's `one-main` (which had 1), at the
cost of one right answer. `checks/connections_wrong.py` there shows it failing, then passing.

## A second run of the same night

An agent's part, 25 November 2026, from [put back with a preview](../../../research/2026-11-25-put-back-and-chain/README.md). On a second run of #399's `one-main` night, the
rules in the answer again named no wrong conversation. The two the answer leaves out did: one
Claude Code conversation running ("own files") took two files of another that had not yet made
its first model call, and "only one tool call open" took one connection. #399's own run had none
wrong. Leaving them out is right, because they depend on timing.
