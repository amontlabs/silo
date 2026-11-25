# Option A's first run, written out from install to the first morning

Research of 25 November 2026, by an agent, for polarzero. #354 wrote #348's first run as a
transcript. This does the same for option A of #373 (#348 plus the record of model traffic, spend
and a first receipt), with the latest drafts: #370's specs, #373's step 6, #374's spend budgets,
#377's real-night kit and claim corpus, and #378's closed network through a proxy over vsock.
Writing each screen in order shows where the drafts disagree or say nothing. Twelve such gaps are
below.

| File | Holds |
|---|---|
| [`transcript.md`](transcript.md) | The first run, the sign-ins, getting the work in, one night and the first morning, as screens, with each gap where it appears |
| [`checks/catch_rate_under_a.py`](checks/catch_rate_under_a.py) | #377's corpus catch rate with processes taken out, as A has none; output in [`checks/result-under-a.txt`](checks/result-under-a.txt) |
| [`checks/claims_npm_narrowed.py`](checks/claims_npm_narrowed.py) | A repro: #377's claim checker marks a false «All tests pass.» as matched |

Both checks take the `research/` folder of #377's branch as their argument.

## In short

1. **The first SSH has no road under #378's network (gap 1).** Provisioning turns on SSH on the
   network card, the card leads nowhere, and the relay that carries SSH over vsock has to be
   installed over SSH. Recommended: start main on Apple's NAT only until the relay is in, then
   start it again on the dead-end card, before any installer runs. That goes against the letter
   of `network-stack` for two minutes, so it is a new entry for polarzero,
   `way-in-before-the-relay`, with two other options.
2. **#373's first screen names `codex` as the author of a deletion and of a paste (gap 12).** A has
   no processes, and #373's own spec says which conversation changed a file is not claimed. The
   lines should name no program, with the tool call that names the path beside them as an opinion.
3. **#377's checker would mark #373's own false claim as matched (gap 8).** Its narrowed-run pattern
   does not know `npm test -- test/auth`. So «All tests pass.» after a failed whole suite comes out
   matched, shown as checked. The repro exits 1 on #377's tree.
4. **Without processes, the receipt catches fewer false claims (gap 7).** On #377's corpus test
   half, the receipt's own warnings catch 3 of 30 instead of 7. With `claims@0.3`, 10 instead of 13.
5. **The first morning's only way back costs the other agent's night (gap 9).** #373 moves `look`
   and `put-back` out of A. Bringing back the folder Codex deleted means restoring all of main to
   02:40, and Claude Code's three hours after it.
6. **The first run ends with no repository in main (gap 2).** #373 moves import out of A, but import
   is a copy over SSH and does not need the reader. Repositories copied by hand keep
   `git@github.com:` remotes, which main has no key for.
7. **Codex's commands may be unreadable from traffic (gap 6).** Codex 0.172.0 sends commands as
   JavaScript to one `exec` tool, which returns JSON (#374). #377 reads exit codes with a text
   pattern. The receipt's commands and its last whole test run can be empty for Codex.
8. **The build of Claude Code main gets has never run through the boundary (gap 11).** Every trial
   ran the npm package. Main gets `claude.ai/install.sh`'s build, through `HTTPS_PROXY`, which is
   untested, as is Codex's WebSocket through a `CONNECT` proxy.
9. **The network question is real again, and "not checked" is wrong (gap 3).** Under A every
   connection is recorded, and with #378 a closed main is possible. Open stays the default in A,
   because A has no rules or asks to let a blocked `npm install` through at night.
10. **The first morning opens on about twenty first-seen host alerts (gap 4)**, the paste site among
    them. Hosts the installers reached should count as seen, and the rest merge into one line.
11. **A program that ignores the proxy leaves no trace (gap 10).** It fails as if offline. The Mac
    end of the card can count what it drops.
12. **A guest clock behind the Mac breaks every certificate the boundary signs (gap 5).** #378
    found it and flagged it on #348's `mac-kept-awake`. #373's step 6 does not date its
    certificates back.

## The gaps, by entry

Notes were added to each entry below, as "Agent, 25 November 2026", under a section "From option
A's first-run transcript". One entry is new. No verdict, answer or list was changed.

| Gap | Entry | What the note says |
|---|---|---|
| 1 | `way-in-before-the-relay` (new, engine, after `network-stack`) | How the first run reaches main before the relay; three options, Apple's NAT for the first start recommended |
| 2 | `files-imported-from-your-mac` | Keep it in A; it does not need the reader. SSH remotes and the token `gh` leaves in main |
| 3 | `safe-fresh-install` | Under A with #378, open is recorded and closed is possible; open stays the default until rules and asks exist |
| 4 | `alert-only-on-a-change` | The first night's first-seen hosts: count the installers' hosts as seen, merge the rest into one line |
| 6, 11 | `reading-each-agent-program` | The check covers the shape of tool calls and results; the install script's build and the proxy have not run |
| 7 | `what-a-receipt-holds` | What "still running: not known yet" costs on the corpus |
| 8 | `agent-summary-beside-receipt` | The false matched; the proposed text should name "unbacked" and match only on the whole run |
| 9 | `restore-to-a-checkpoint` | Keep `look` and `put-back` in A |
| 10 | `network-place` | Count the frames the dead-end card drops |

Gaps 5 and 12 have no note: 5 is on #348's draft entry `mac-kept-awake` through #378, and 12 is a
change to #373's spec page, not to an entry. Gap 12's fix is one line in #373's `specs.md`,
"Step 5: the record".

## Contradictions between the drafts

| Drafts | They disagree on | The transcript follows |
|---|---|---|
| #348 `network-before-rules`, #370 step 2 / #373 step 2, #378 | An open main is "not checked, held or recorded" (#348, #370); main is on gvproxy from day one and test 07 blocks (#373); the dead-end card needs no gvproxy (#378) | #378, with #373's boundary taking its connections from the proxy |
| #373 step 5, within itself | The first screen names `codex` on a deletion and a paste; the next paragraph says no file is attributed | No program named; the link as an opinion |
| #373 `agent-summary-beside-receipt` / #377 | Three answers for a claim, or four with "unbacked" | Four |
| #373 / #370 step 5, #354 | `look`, `put-back` and import out of A, or in the first build | In, recommended |
| #354 / #370 | Six first-run questions, or three | #370's three, the first one rewritten |
| #354 / #370 | A checkpoint every 10 minutes, or every 5 | #370's 5 |

## For polarzero to rule

- `way-in-before-the-relay`: Apple's NAT for main's first start, then the dead-end card. Recommended.
- Keep `files-imported-from-your-mac`, `silo look` and `silo put-back` in option A, against #373's
  build order. Recommended.

## What was not verified

- Nothing ran on a Mac. Every time in the transcript is #354's estimate or #370's spec plus steps
  nobody measured.
- The catch rates are #377's corpus, rebuilt from what reporters wrote, with processes taken out.
  They measure which kinds of claim the checker reads, not how often A would catch a real agent.
- Whether Codex's `exec` tool results carry an exit code a pattern can read was not run: it is read
  from #374's stand-in provider, which plays Codex's tool format.
- Whether the native Claude Code build honours `HTTPS_PROXY`, `NODE_USE_ENV_PROXY` and
  `NODE_EXTRA_CA_CERTS` was not tried; no container here has it.
- Whether Apple's provisioning can start a program, which would remove gap 1, was not found either
  way.

## Sources

- The plan at `1b8f191`.
- Drafts #348, #354, #370, #373, #374, #377 and #378 of this repository, read at their heads of
  25 November 2026.
- #377's `corpus/catch_rate.py` and `proto/claims.py`, run unchanged apart from the processes taken
  out of the evidence.
