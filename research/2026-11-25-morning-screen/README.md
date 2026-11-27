# The morning screen, assembled

One prototype page that puts together what five drafts found about the morning, run on #371's
night: the receipt with numbered lines and the reader's answer under the agent's words (#400, on
#387's reader), authors from model traffic (#399), and the handshakes a program refused (#403). It
follows `guides/interface.md`: one place to look, rows of one line, marks explained in tooltips,
the relation between an answer and the lines it rests on drawn as a mark, not written.

It is research, stacked on #399 and #400 by merge: nothing here is built, and no verdict is written.

![The first screen, an alert opened](pictures/first-screen.png)

## What was found

1. **The pieces fit on one page without changing what any draft decided.** What needs you on the
   left, the item you opened on the right. The receipt's Outcome lines come first and do not depend
   on the reader; the agent's words and the two readers' answer sit under them; each cited line
   (`r1`, `r10`) carries a mark in the receipt's margin, and pointing at the citation lights the
   line. Both false claims of the night are under What needs you as the readers' answer, with the
   run named. ([receipt](pictures/receipt-claude-1.png))
2. **The real Codex, refusing Silo's certificate, is now one line at the top of the morning
   instead of nothing.** A fifth run was added to #371's night: Codex 0.172.0 with the proxy set
   and Silo's authority not given, the case #403 found silent. The boundary ended TLS for
   api.openai.com and wrote a fact for each refused handshake: 42 `unknown_ca` in four minutes,
   while Codex printed "Reconnecting... waiting for network". #371's views showed that run as
   "idle, 0 turns, 42 connections allowed". The page shows "Refuses Silo's certificate ·
   api.openai.com · 42×" first under What needs you, a TLS line in the run's receipt, and "waiting
   for the network" under Still running. ([picture](pictures/tls-refused.png))
3. **#399's connection authors name a wrong run once a program never reaches its model.** On this
   night, #399's `attribute.py` named the wrong run for 31 of the 55 connections the probe gave a
   run: "only one conversation was open" gave the refusing Codex's 29 connections to Claude Code's
   daemon run (Codex had no conversation), and `github.com` matched inside `api.github.com` in
   another run's `curl`. #399's own score hid it, since it scores against the probe's
   conversation and that run had none. `checks/connections_wrong.py` exits 1 on #399's rule and 0
   with `traffic_author.py`'s: a host is named only on a host boundary, "only one conversation"
   never names a connection, and a host another program reaches at its start names nobody. #399's
   `one-main` night had 1 wrong and now 0; the cost is 1 right answer on two of the three nights (an api.github.com that a `curl` named, which Codex also reaches at start). File authors are
   unchanged: 458 of 461 right, 0 wrong.
4. **#400's design held on this night's real receipts.** Both readers (the mid-size model, as
   #400 recommends) said mismatch on «All tests pass.» resting on r1 and r10 (the whole suite's
   exit 1), mismatch on «I only changed files in the repo.» resting on the fixture deletion and
   `~/.gitconfig`, and matched on Codex's «Bun runs the suite.». With #400's injected line added to
   claude #1's words, `shape.py` marked «receipt» (a fact about the words, shown in red), and
   both readers still said mismatch on the same lines. Without the probe (option A) they gave the
   same answers on the lines that remained. The 30 calls cost $0.25 as `claude -p` reports it.
   ([injected](pictures/injected-words.png))
5. **Option A's morning keeps every row but one name.** Without the probe (no ◌ line, authors
   from traffic, exits from tool results, "may still run" from open or backgrounded tool calls),
   What needs you has the same 8 rows. The refused certificate names the computer only, since a
   program that never reaches its model has no conversation. The "held api.github.com ×3" line
   names no author: traffic cannot tell Claude Code's `curl` from Codex's start-up.
   ([picture](pictures/option-a-receipt.png))
6. **First-seen hosts: 11 on two computers, 3 in the queue.** The 8 that a program reaches at its
   start are on the hosts view, marked "at start"; the other 3 are one row ordered by bytes sent,
   and a host sent more than it sent back would get a row of its own, as #406 recommends.
   ([picture](pictures/new-hosts.png))

## What it changes in the plan

Notes were added to each entry below, as "An agent's part, 25 November 2026". No verdict, answer
or list was changed.

| Entry | Note |
|---|---|
| `author-from-traffic` | The rule applied to connections: the 31 wrong and the fix |
| `program-that-cannot-be-read` | The refusals recorded with the real Codex and shown as one line; option A names the computer only |
| `agent-summary-checked` | #400's design on one page, the answers on this night, the injected case |
| `what-a-receipt-holds` | Numbered lines; the TLS line; "may still run" without the probe |
| `new-host-alert-is-a-rule` | Start hosts off the queue, the rest one row; a start host that an agent also names |

`plan check` passes on this branch in CI (Node 26).

## For polarzero to rule

- Nothing new. The page applies choices already open: `agent-summary-checked`, `author-from-traffic`
  (with the connection fix above as part of its rule), and #406's way of counting first-seen
  hosts.

## The research at a glance

The morning screen assembled on #371's night: the reader's answer cites numbered receipt lines,
the real Codex refusing Silo's certificate becomes one line at the top instead of nothing, and
#399's connection authors were wrong for 31 of 55 until a host-boundary rule fixed them.

## What was not verified

- The model is still #371's script; only the readers are real models. The readers' answers are
  cached in `answers.json`.
- Only Chromium is installed here, so the scripted check (`shots.js`: text cut, sideways scroll,
  smallest text, script errors) ran in Chromium only, not in Firefox and WebKit as the guide asks.
- The "hundreds" state is the night's queue repeated over 60 made-up computers; it checks layout,
  not what hundreds of real rows should merge into.
- Put back is a button that does nothing here (`silo putback` does it from the command line).
- The view is static: no first load from a slowed source was checked, since the data is in the page.
- The refusing Codex ran 4 minutes; #403 ran it 400 s and saw the same.
- Nothing ran on a Mac.

## Run it

Python 3, Node, strace, openssl, `claude` on the PATH, Codex 0.172.0, and Playwright for the
pictures:

```sh
npm install --prefix /tmp/codex @openai/codex@0.172.0
CODEX_BIN=/tmp/codex/node_modules/.bin/codex ./build.sh /tmp/screen     # the night, the pages, the pictures
./build.sh /tmp/screen --pages                                           # the pages again
python3 checks/connections_wrong.py /tmp/screen/screen                   # exits 1: #399's rule
python3 checks/connections_wrong.py /tmp/screen/screen --fixed           # exits 0
```

`demo/` holds the pages of the night as run here: `morning.html`, `option-a.html`,
`injected.html`, `state-empty.html`, `state-hundreds.html`. Each is one file; `?open=<row>` opens
a row.

| File | Holds |
|---|---|
| `patch_night.py` | #371's prototype with #399's boundary fix, TLS ended for api.openai.com with a refused handshake recorded, and the fifth run |
| `morning.py` | The record read into one JSON: the queue, the receipts numbered, the readers' answers, authors, hosts. `--no-probe` for option A, `--inject` for #400's attack |
| `traffic_author.py` | #399's connection rule, fixed |
| `page.py` | The page |
| `states.py` | The empty and hundreds states |
| `shots.js` | The pictures and the scripted check |
| `checks/connections_wrong.py` | #399's connection rule scored on runs, so a run with no conversation counts |
