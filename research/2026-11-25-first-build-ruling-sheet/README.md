# The first build: one sheet to rule from

An agent's page of 25 November 2026, for polarzero. It recounts option A (#373: a macOS main with
the record of model traffic) against #348 and option B (#381: a Linux main on the Mac), with
everything the drafts found since #373: #378, #380, #381, #382, #393, #397, #398, #399, #402, #403,
#407, and the newest #409 and #411. It lists the open decisions each option needs, with the second
opinion each already has, and the Mac tests that still block. It changes no entry and no verdict.

Counts come from following `needs` through the plan with #381's own script
(`research/2026-11-25-linux-main/tools/count.py`), run on #391's tree with #397 to #408's entries
laid on top. Every entry named here is in #391 or in the draft named beside it, so it shows in the
Workshop once that draft is merged.

## In one paragraph

Rule A. The research since #373 removed most of what made A risky and added nothing that favours B
for the first night. #378 and #380 give A a closed network without gvproxy or a stack of Silo's
own, so Mac test 07 no longer blocks it; #402 gives the first SSH a road that keeps "never Apple's
NAT"; #403 ran the native Claude Code and Codex's WebSocket through that network; #399 names the
conversation behind 453 of 461 file changes from traffic alone, so A's first morning can say who
did what without processes; #393 gives A a setting for a Claude plan that no longer leans on a
missing fallback. What A still rests on is the Mac itself: macOS 27, Apple's provisioning (tests 00,
01), a clone of a running disk (04) and Silo's authority in the Keychain (21). #381's ordering keeps
B ready if one of those fails: build step 1, a Linux computer and the boundary step first, then
choose with the four tests in hand.

## What changed since #373, and what it does to A and B

| Draft | Found | For A | For B |
|---|---|---|---|
| #378 | A closed network with no stack: a card that answers ARP for one router and drops the rest, and a proxy at 127.0.0.1:3128 over vsock. Claude Code worked through it | Test 07 (gvproxy under a macOS computer) stops blocking. `network-stack` gets a third option, recommended for build 1 | Same card works; B already used vsock |
| #380 | gvproxy patched (320 lines, 21 hostile cases) | gvproxy for an open main later, not build 1 | same |
| #381 | B specified at #370's level; A 87 open decisions, B 96 | Recommends A, built in an order that leaves the choice open | B can start the day it is taken, on a Linux host with KVM |
| #382 | 12 gaps in A's first run; the first SSH has no road (`way-in-before-the-relay`) | Opens the entry; recommends keeping import, `look` and `put-back` in A | |
| #402 | The card's Mac end makes one TCP connection to main's port 22 at its 169.254 address; 13 of 13 escapes blocked | Answers `way-in-before-the-relay` without Apple's NAT. Rests on three Mac facts: main takes a 169.254 address with no DHCP, announces it by ARP, and Remote Login listens on it | B goes in over vsock and needs none of it |
| #403 | Native Claude Code and Codex's WebSocket through the closed network | Works; the boundary must record TLS refusals, since the proxy logs them as allowed | same |
| #393 | `claude-key-or-plan`: a key is read, a plan is passed on unread | Replaces `anthropic-reads-intermediate`'s fallback, which A had left with nothing | same |
| #407 | A plan passed unread shows no window; Claude Code's own report matched 38 of 38 | `plan-limit-reached` cannot fire on an unread plan; `program-reports-its-own-spend` fills it | same |
| #399 | `author-from-traffic`: 453 of 461 changes named, none wrong | A's first screen can name an author (#382's gap) | B has the probe too |
| #397 | Main's clock falls behind by the whole sleep; 5 s breaks HTTPS and signed commits | `main-clock-kept` belongs with `mac-kept-awake` | same, Linux guest |
| #398 | libkrun's write hook, 30 to 40 ns a write; a libkrun bug fixed | | B's `delta` checkpoint is built |
| #409 | Three ways to keep a computer's link across a gvproxy restart; a second `VFKT` cuts a computer off silently | Only if gvproxy is used | same |
| #411 | Clones give up compression: 4.3 days back on a 512 GB Mac against 26.3 with Silo's own files | Challenges `checkpoint-per-system`'s first sentence, as #381 did | same |

## The counts

| | A | B |
|---|---|---|
| Features | 19 (#348's 17 without `a-path-from-a-checkpoint` and `files-imported-from-your-mac`, plus `model-traffic-read`, `model-calls-in-the-record`, `spend-per-conversation` and a first receipt) | 20: the same with a Linux main, plus `processes-in-the-record` |
| Open decisions reached through `needs` | 87 | 96 |
| Only in this option | `first-run-time-macos` | `main-on-linux`, `linux-vm-engine`, `libkrun-patches`, `engine-patch-review`, `work-on-top-of-libkrun`, `processes-from-inside-linux`, `blank-computer`, `containers-on-macos`, `storage-between-computers`, `x86-linux-computers` |

The counts are those of #381: the entries added since are not yet named in any feature's `needs`.
The research says option A needs these too, so rule them with the rest:

| New open decision | From | Recommended answer | Needs |
|---|---|---|---|
| `way-in-before-the-relay` | #382, #402 | The card's Mac end opens one connection to main's SSH port; Apple's NAT for the first start only if the Mac test fails (#402 supersedes #382's and #391's Apple's NAT) | `network-stack` |
| `claude-key-or-plan` | #393 | Each computer is set for Claude with a key (read) or a plan (passed on unread) | `anthropic-reads-intermediate`, `subscription-and-key-alike` |
| `author-from-traffic` | #399 | Name the conversation from the tool call that sent, named or wrote the path; else "while these ran" | `the-chain`, `reading-each-agent-program` |
| `main-clock-kept` | #397 | Step main's clock on each wake, check every minute | `mac-kept-awake` |
| `program-reports-its-own-spend` | #407 | Offer to point the program's own report at Silo where traffic passes unread, shown as the program's word | |
| `record-damaged-at-start` | #386, #394 | Keep recording in a new segment and write a fact naming the damage, with #394's after-head rule | `changes-to-the-record`, `enforcing-and-recording` |
| `record-head-off-the-mac` | #394 | A Git repository with a protected branch for one person, a head every minute | `changes-to-the-record`, `record-file-format` |

Four more that the drafts propose for build 1 are edits, not decisions of their own: the `device`
author in `who-made-each-change` (#370, #386), refusing what the boundary cannot read exactly in
`forwarded-byte-for-byte` (#401), a limit in `enforcing-and-recording` (#395), and keeping import,
`silo look` and `silo put-back` in A (#382), which adds `a-path-from-a-checkpoint`,
`files-imported-from-your-mac` and `putting-back-a-path` back to A's count.

## Mac tests that still block

From #390's kit (`./kit run blocking` runs 00, then 07, 21 and 35), updated for what came since.

| Test | What it settles | Blocks | Since #373 |
|---|---|---|---|
| 00 | macOS 27 downloads and installs | A at all; `mac-silo-needs` | polarzero's Mac ran 26.5 on 21 November: update it first |
| 01, 10, 13, 14 | Apple's provisioning sets main up with nobody at the screen | A's step 2; `main-on-macos-first`, `computer-image`, `moving-in-on-macos` | none |
| 21 | Silo's authority in main's System keychain over SSH | Reading model traffic in A without each program's own setting (a fallback exists) | #403: every program also takes `SSL_CERT_FILE` or the system store |
| 04 | A clone of a running disk is one moment | `checkpoint-per-system` (a pause is the fallback) | #411 argues for Silo's own files over clones |
| 07, 35 | gvproxy under a macOS computer, sealed | Only if gvproxy carries build 1 | #378: no longer blocks A |
| #402's three facts | Main takes a 169.254 address with no DHCP server, announces it by ARP, and Remote Login listens on it; also whether Setup Assistant needs the internet while provisioning (if so, only Apple's NAT works for the first start) | `way-in-before-the-relay` as recommended | new; the Mac-kit thread is adding them (backlog 28) |
| 37 | The guest's clock after a wake | `main-clock-kept`, `mac-kept-awake` | #397 |
| 34 | A whole night on the subscription through the boundary | #373's last step | none |

B needs no Mac test before its first night apart from L1 (the helper on HVF), L2, L5 and #359's 06,
09, 20; everything else runs on a Linux host with KVM (#381's K00 to K16).

## The ruling, in order

1. **Which main first.** A, built in #381's order (step 1, a Linux computer for the boundary, step
   6, then the macOS main once tests 00, 01 and 21 pass). The one question that would turn it: does
   the first night need Xcode or macOS apps in main? If not, B has fewer unknowns.
2. **The 19 features of A**, to take: nothing is built until they are (`AGENTS.md`). Add the three
   that #382 keeps in A if you agree with it.
3. **The decisions below**, in one sitting, top to bottom. 56 have #352's second opinion and 19
   have #373's; the rest carry their own answer with no second opinion. The engine's twelve marked
   "agents" in #352 can be taken by agents with a judge under `owners-part-on-engine` once you say
   so; no agent writes a verdict while you are away.
4. **The seven new decisions above.**

### Every open decision of the first build

Sorted by how many open entries rest on each, through `needs`. "both" means A and B need it.

| Decision | Part | Open entries resting on it | Option | Recommendation | Mac tests | Since #373 |
|---|---|---|---|---|---|---|
| `engine-shape` | engine | 143 | both | Take as written (#352) |  |  |
| `main-on-macos-first` | computers | 134 | both | Take as written; fix the notes (#352) | 00 01 13 07 35 | #381: B specified; keep A and leave the choice until tests 00, 01, 07, 21 |
| `engine-state` | engine | 126 | both | Edit (#352) |  |  |
| `engine-inputs` | engine | 124 | both | Edit (#352) |  |  |
| `how-the-engine-is-built` | engine | 123 | both | Edit (#352) |  |  |
| `numbered-events` | engine | 123 | both | Edit (#352) |  |  |
| `call-ids` | engine | 122 | both | Edit (#352) |  |  |
| `core-dependencies` | engine | 122 | both | Edit (#352) |  |  |
| `core-knows-no-agents` | engine | 122 | both | Edit (#352) |  |  |
| `deterministic-core` | engine | 122 | both | Edit (#352) |  |  |
| `engine-limits` | engine | 122 | both | Edit (#352) |  |  |
| `engine-source-checks` | engine | 122 | both | Edit (#352) |  |  |
| `formats-carry-versions` | engine | 122 | both | Edit (#352) |  |  |
| `tigerstyle-by-place` | engine | 122 | both | Edit (#352) |  |  |
| `who-can-call-the-engine` | engine | 122 | both | Edit (narrowed) (#352) | 20 07 |  |
| `files-through-a-channel` | computers | 121 | both | Edit (#352) |  |  |
| `computer-image` | computers | 120 | both | Split (#352) | 00 10 13 23 |  |
| `engine-reconciles` | engine | 120 | both | Edit (#352) |  |  |
| `network-stack` | engine | 119 | both | Edit (#352) | 07 35 36 | #378/#380: dead-end card plus a proxy over vsock for build 1, gvproxy patched later; test 07 no longer blocks A. #409: where gvproxy is kept, the helper re-points the link after a restart; a second `VFKT` cuts a computer off silently |
| `how-effects-work` | engine | 118 | both | Edit (#352) |  |  |
| `mac-silo-needs` | computers | 117 | both | its own answer; no second opinion | 00 01 10 13 |  |
| `network-before-rules` | boundary | 117 | both | its own answer; no second opinion |  | #378: closed from the first start without a stack |
| `neutral-core` | computers | 117 | both | Edit (#352) | 12 |  |
| `one-main-or-several` | computers | 117 | both | Take as written (#352) |  |  |
| `first-run-time-macos` | computers | 116 | A only | Edit (#352) | 00 13 14 23 |  |
| `record-path-and-core` | record | 103 | both | Edit (#352) |  |  |
| `core-state-stays-small` | engine | 102 | both | Replace the answer (#352) |  |  |
| `engine-saved-state` | engine | 101 | both | Edit (#352) | 06 31 |  |
| `silo-is-light` | decisions | 93 | both | Edit (#352) | 16 |  |
| `own-cost-targets` | checking | 92 | both | Edit (#352) | 06 16 36 |  |
| `word-gap` | record | 92 | both | Edit (#352) |  |  |
| `kinds-of-lines` | record | 91 | both | Edit (#352) |  |  |
| `changes-to-the-record` | record | 88 | both | Take as written (#352) |  |  |
| `record-speed-cost` | record | 88 | both | Edit (#352) | 06 16 |  |
| `word-line` | record | 88 | both | Take as written (#352) |  |  |
| `enforcing-and-recording` | record | 87 | both | Edit (#352) |  | #395: a limit after which a request waiting for the record is refused |
| `record-overhead-limit` | record | 87 | both | Edit (#352) |  | #398: the write hook costs 30 to 40 ns a write |
| `sources` | record | 87 | both | Edit (#352) |  |  |
| `who-made-each-change` | record | 87 | both | Edit (#352) | 37 | #370/#386: add the `device` author; #399: `author-from-traffic` names the conversation under A |
| `sources-number-their-lines` | record | 86 | both | Edit (#352) |  |  |
| `events-until-the-recorder` | record | 85 | both | Edit (#352) | 06 |  |
| `git-history-inside` | record | 85 | both | Take as written (#352) |  |  |
| `measuring-the-store` | record | 85 | both | Edit (narrowed) (#352) |  |  |
| `observing-and-waiting` | record | 85 | both | Edit (#352) |  |  |
| `processes` | engine | 83 | both | its own answer; no second opinion |  | #409: the helper owns each computer's link |
| `where-rules-run` | engine | 71 | both | its own answer; no second opinion |  |  |
| `documented-surfaces` | agent-programs | 67 | both | Edit (#352) |  |  |
| `ending-the-encrypted-connection` | traffic | 61 | both | Take with an edit (#373) |  |  |
| `fork-shares-disk-blocks` | computers | 60 | both | Take as written (#352) |  |  |
| `which-process-holds-the-boundary` | engine | 57 | both | its own answer; no second opinion |  |  |
| `unmodified-agent-programs` | agent-programs | 56 | both | Take as written; add notes (#352) | 11 |  |
| `main-set-up-for-agents` | agent-programs | 54 | both | its own answer; no second opinion |  |  |
| `program-that-cannot-be-read` | traffic | 45 | both | Take with an edit (#373) |  | #384 local models channel; #401; #403 a program refusing Silo's certificate looks allowed to the proxy; #407 |
| `reading-each-agent-program` | traffic | 45 | both | Take with an edit (#373) | 21 34 | #399: Codex 0.172.0 sends tools as one `exec` custom tool; #403: native Claude Code and Codex WebSocket work through the proxy |
| `anthropic-reads-intermediate` | traffic | 44 | both | Take with an edit (#373) |  | #393: its fallback is replaced by `claude-key-or-plan` (key read, plan passed unread) |
| `logins` | agent-programs | 44 | both | Take as written (#373) |  |  |
| `how-spend-is-shown` | traffic | 43 | both | Take as written (#373) |  | #407: a Claude plan passed unread shows no window; `program-reports-its-own-spend` matched 38 of 38 |
| `subscription-and-key-alike` | traffic | 43 | both | Take as written (#373) |  | #393, #407: a plan read and a plan passed unread no longer look alike |
| `what-silo-changes-in-traffic` | traffic | 43 | both | Take #364's rewording (#373) |  |  |
| `file-changes-from-outside` | record | 42 | both | Edit (#352) | 05 14 17 18 19 39 | #381 (journal replay, B), #390 (LZVN/LZFSE decode wrong bytes silently), #401/#410 (ext4 reader fixes) |
| `what-traffic-gives` | traffic | 42 | both | Take with an edit (#373) |  |  |
| `forwarded-byte-for-byte` | traffic | 41 | both | Take with an edit (#373) |  | #401: six spellings and two framings carried calls past the budget; refuse what is not read exactly (patch exists) |
| `provider-sees-silos-connection` | traffic | 41 | both | Take as written (#373) | 34 |  |
| `recognising-a-conversation` | traffic | 41 | both | Take with an edit (#373) | 34 | #376 sets the word `conversation` again (owner only); #399 |
| `hidden-reasoning-stays-closed` | traffic | 40 | both | Take as written (#373) |  |  |
| `checkpoint-per-system` | going-back | 37 | both | Edit (#352) | 03 04 15 31 32 33 | #381/#398/#411: the first sentence excludes the write-log `delta`; #411: clones give up compression (4.3 against 26.3 days on 512 GB) |
| `checkpoint-storage` | going-back | 37 | both | Edit (#352) | 33 38 | #411: Silo's own files can move to an external disk |
| `meanings-for-computers` | decisions | 37 | both | Edit (#352) |  |  |
| `pause-every-computer` | computers | 35 | both | Edit (#352) |  |  |
| `space-the-engine-keeps` | engine | 35 | both | its own answer; no second opinion | 06 33 38 | #389, #365: floor and reserve file |
| `what-stop-ends` | computers | 35 | both | Edit (#352) | 12 |  |
| `network-place` | views | 30 | both | Take as written (#373) |  |  |
| `restore-saves-first` | going-back | 28 | both | Edit (#352) |  |  |
| `why-destructive` | rules | 27 | both | Take as written (#352) |  |  |
| `what-a-restore-leaves` | going-back | 14 | both | Edit (#352) |  |  |
| `what-a-receipt-is` | views | 10 | both | Take as written (#373) |  |  |
| `conversation-text-kept` | record | 9 | both | Take as written (#373) |  |  |
| `kinds-of-other-actions` | rules | 8 | both | its own answer; no second opinion |  |  |
| `when-the-engine-stops` | engine | 7 | both | its own answer; no second opinion | 09 36 |  |
| `computer-reached-over-ssh` | agent-programs | 6 | both | Edit (#352) | 02 08 11 14 | #402: the way in before the relay; the relay starts `sshd -i` over vsock |
| `what-a-receipt-holds` | views | 6 | both | Take with an edit (#373) |  | #399: authors from traffic; #400: the reader's answer under the words |
| `reaching-a-computer` | agent-programs | 5 | both | Edit (#352) | 08 20 | #402 |
| `agent-summary-beside-receipt` | views | 4 | both | Take with an edit (#373) |  | #387/#400: `agent-summary-checked`, a model reader as an extension |
| `record-queried-with-ordinary-tools` | record | 3 | both | its own answer; no second opinion |  |  |
| `mac-kept-awake` | computers | 2 | both | its own answer; no second opinion | 37 16 | #397: `main-clock-kept`, the clock stepped on wake and checked each minute |
| `runs-outlive-the-terminal` | agent-programs | 1 | both | its own answer; no second opinion | 02 37 |  |
| `thinning-checkpoints` | going-back | 1 | both | its own answer; no second opinion | 31 33 | #389: 2 to 4 days on a 512 GB Mac; #411: more with compressed checkpoints |
| `linux-vm-engine` | engine | 33 | B only | its own answer; no second opinion |  |  |
| `processes-from-inside-linux` | record | 33 | B only | its own answer; no second opinion |  |  |
| `blank-computer` | recipes | 28 | B only | its own answer; no second opinion |  |  |
| `storage-between-computers` | computers | 27 | B only | its own answer; no second opinion |  |  |
| `main-on-linux` | computers | 11 | B only | its own answer; no second opinion |  |  |
| `containers-on-macos` | computers | 6 | B only | its own answer; no second opinion |  |  |
| `engine-patch-review` | engine | 6 | B only | its own answer; no second opinion |  |  |
| `libkrun-patches` | engine | 6 | B only | its own answer; no second opinion |  |  |
| `work-on-top-of-libkrun` | engine | 6 | B only | its own answer; no second opinion |  |  |
| `x86-linux-computers` | computers | 6 | B only | its own answer; no second opinion |  |  |

## Refreshed 26 November: drafts #413 to #433

Read on 26 November at about 16:00 Paris. None of them changes the recommendation (A, in #381's
order) or the counts above, since none adds a need to a first-build feature. What they change for
the first build:

| Draft | Found | For the first build |
|---|---|---|
| #415 | Mac kit tests 40 to 49 for what the drafts of 25 November left to a Mac | Step 2 gains 40 to 43 and 47 (#378's relays over vsock, #380's gvproxy, `127.0.0.1` through gvproxy, libkrun); step 4 gains 46 and 48; step 5 gains 45. Run `blocking`, then `step2`, as before |
| #417 | #395's seven boundary conflicts settled, each shown failing first | `secret-placeholder` gets new sentences (with #378's proxy a program that ignores it reaches nothing); `enforcing-and-recording` unchanged: a limit, then refuse |
| #418 | Codex read in full: its `exec` scripts report what they ran if a setting is on | New `code-mode-calls-reported`: main's set-up turns Codex's report on; A needs it for #399's attribution of Codex's changes |
| #421 | The morning screen assembled on #371's night, with a real TLS refusal | Edits `main-set-up-for-agents` and `runs-outlive-the-terminal`, both in A |
| #425 | TLS refusals recorded per program at #395's boundary, one alert and one morning line | Notes on `ending-the-encrypted-connection`, `program-that-cannot-be-read`, `alert-only-on-a-change` |
| #427 | Claude Code's own report checked against the connections Silo sees | `program-reports-its-own-spend` now "checked against what the boundary sees" |
| #414 | Every prototype that keeps state audited for Mac durability | `what-a-budget-promises`: a call cut off by a restart counts at its reserve |
| #429 | A size check misses 64% of corrupted LZFSE blocks and 69% of LZVN | Challenges #390 and #370: decode with Apple's `libcompression` on the Mac and judge the reader against what macOS reads; a hash per checkpoint block as a new choice in `going-back` |
| #431 | One disk budget for checkpoints, the record, recordings and the bin | Notes on `low-disk`, `thinning-checkpoints`, `record-size-limit`: half of what is free, keeping 50 GiB |
| #433 | Persistence an agent leaves, found by the record and undone by put-back | New `putting-back-a-new-path` and `restart-after-a-put-back`, for when `put-back` is in A (#382) |

Later builds only: #413 (`summary-reader-model`), #419 (a team budget in a Git branch), #420 (the
ask on a phone), #422 (`secret-shaped-values`), #423 (exposed, rescored), #424
(`uploads-to-your-places`), #426 (forked conversations), #428 (`google-plan-counted-unread`), #430
(gvproxy's upstream series), #432 (the shared host's leaks).

## What was not checked

- The counts follow `needs` only. An entry a draft says the first build needs but that no feature
  names is listed above by hand, from the drafts' READMEs.
- #397 to #408 were laid over #391 file by file. Where two drafts edited the same notes (#399 and
  #403 on `reading-each-agent-program`; #401, #403 and #407 on `program-that-cannot-be-read`), the
  integration branch settles the text; this page only reads their answers.
- Nothing here ran on a Mac.
