# Research

The reports and prototypes behind the plan's notes, written by agents from 24 to 26 November 2026
while polarzero was away. Each folder is one subject, dated, with a README that says what was
found, what it changes in the plan by entry id, and what was not verified. They are evidence, not
product: nothing here is built into Silo, and a report is not edited after its date.

Read a report only when a note of the plan cites it. Three things to know before trusting one:

- **Every number is a Linux prototype result.** No report ran on a Mac, and none ran on a real
  model's API key unless it says so. The Mac kit (`mac-test-kit/`) is how a Mac checks them.
- **Reports written the same day sometimes disagree.** The plan says which reading holds: each
  entry's answer, and its notes under "Settled in the consolidation". A report is never the
  authority over the plan.
- **`2026-11-25-integration` and `2026-11-25-integration-2`** record how the drafts were merged,
  and the fix-up each needed.

| Folder | Draft | Subject |
|---|---|---|
| [`2026-11-24-first-build.md`](2026-11-24-first-build.md), [`2026-11-24-first-build-milestones.md`](2026-11-24-first-build-milestones.md) | #348 | The smallest first build, and its milestones |
| [`2026-11-24-agent-programs-follow-ups/`](2026-11-24-agent-programs-follow-ups/) | #369 | Agent programs: the follow-ups of the first survey (24 November 2026) |
| [`2026-11-24-checkpoint-engine/`](2026-11-24-checkpoint-engine/) | #365 | The checkpoint engine, prototyped on Linux |
| [`2026-11-24-destructive-calls/`](2026-11-24-destructive-calls/) | #353 | Destructive calls through allowed APIs |
| [`2026-11-24-every-agent-program/`](2026-11-24-every-agent-program/) | #367 | Every agent program through the boundary (24 November 2026) |
| [`2026-11-24-experience/`](2026-11-24-experience/) | #354 | The first run and the daily loop: what using Silo is like |
| [`2026-11-24-first-build-and-the-record/`](2026-11-24-first-build-and-the-record/) | #373 | The first build, weighed against what sets Silo apart |
| [`2026-11-24-first-build-decisions/`](2026-11-24-first-build-decisions/) | #375 | First-build decisions: second opinions, and what to rule |
| [`2026-11-24-first-build-specs/`](2026-11-24-first-build-specs/) | #370 | Build specs for the first build |
| [`2026-11-24-gap-fixes/`](2026-11-24-gap-fixes/) | #355 | Fixes for the gaps in the plan |
| [`2026-11-24-linux-host/`](2026-11-24-linux-host/) | #366 | A Linux host for Silo |
| [`2026-11-24-mac-and-ios-developers/`](2026-11-24-mac-and-ios-developers/) | #361 | Mac and iOS developers in a macOS computer |
| [`2026-11-24-morning-review/`](2026-11-24-morning-review/) | #371 | The morning review, tried on a real night |
| [`2026-11-24-real-pains/`](2026-11-24-real-pains/) | #350 | What people who give coding agents full access actually suffer |
| [`2026-11-24-reconciling-the-drafts/`](2026-11-24-reconciling-the-drafts/) | #364 | Reconciling the drafts of 24 November 2026 |
| [`2026-11-24-record-and-spend/`](2026-11-24-record-and-spend/) | #351 | The record and the budget: what to build so they hold |
| [`2026-11-24-red-team/`](2026-11-24-red-team/) | #358 | How a hostile agent gets past Silo's precautions |
| [`2026-11-24-red-team-fixes/`](2026-11-24-red-team-fixes/) | #363 | Fixes for the top attacks of the red team, built and measured |
| [`2026-11-24-secrets-not-sent-as-is/`](2026-11-24-secrets-not-sent-as-is/) | #356 | Secrets that are not sent as they are |
| [`2026-11-24-small-teams/`](2026-11-24-small-teams/) | #362 | What Silo needs for small teams |
| [`2026-11-24-spend-budgets/`](2026-11-24-spend-budgets/) | #374 | Spend budgets at the boundary, built and measured |
| [`2026-11-25-agent-program-contracts/`](2026-11-25-agent-program-contracts/) | #434 | Agent programs change under Silo: a contract test per release |
| [`2026-11-25-ask-reaches-a-phone/`](2026-11-25-ask-reaches-a-phone/) | #420 | An ask that reaches a phone, end to end |
| [`2026-11-25-attribution-without-processes/`](2026-11-25-attribution-without-processes/) | #391 (integrated) | Attribution without processes: what option A can say from model traffic alone |
| [`2026-11-25-boundary-conflicts/`](2026-11-25-boundary-conflicts/) | #417 | The boundary's seven conflicts, settled |
| [`2026-11-25-boundary-end-to-end/`](2026-11-25-boundary-end-to-end/) | #391 (integrated) | The boundary end to end, on Linux |
| [`2026-11-25-cardend-hardened/`](2026-11-25-cardend-hardened/) | #438 | cardend against a hostile main (25 November 2026) |
| [`2026-11-25-catch-rate/`](2026-11-25-catch-rate/) | #387 | Raising the morning review's catch rate |
| [`2026-11-25-challenged-decisions/`](2026-11-25-challenged-decisions/) | #376 | The challenged decisions, each ready to rule |
| [`2026-11-25-checkpoint-disk-use/`](2026-11-25-checkpoint-disk-use/) | #391 (integrated) | Stretching how far back checkpoints reach on a Mac |
| [`2026-11-25-checkpoint-retention/`](2026-11-25-checkpoint-retention/) | #389 | Checkpoint retention under a disk limit, and simulators per agent in main |
| [`2026-11-25-codex-read-in-full/`](2026-11-25-codex-read-in-full/) | #418 | Codex read in full: code mode, the ChatGPT sign-in, and its WebSocket under a budget |
| [`2026-11-25-credential-brokering/`](2026-11-25-credential-brokering/) | #379 | SSH keys and cloud credentials through the boundary, measured |
| [`2026-11-25-destructive-calls-clouds/`](2026-11-25-destructive-calls-clouds/) | #391 (integrated) | Destructive calls on AWS, Google Cloud, Azure, GitHub and Kubernetes |
| [`2026-11-25-destructive-calls-platforms/`](2026-11-25-destructive-calls-platforms/) | #383 | Destructive calls on Railway, Fly.io, Vercel, Supabase and Cloudflare |
| [`2026-11-25-disk-budget/`](2026-11-25-disk-budget/) | #431 | One disk budget for everything Silo keeps on a Mac |
| [`2026-11-25-durability-on-a-mac/`](2026-11-25-durability-on-a-mac/) | #414 | Durability on a Mac's terms |
| [`2026-11-25-first-build-ruling-sheet/`](2026-11-25-first-build-ruling-sheet/) | #416 | The first build: one sheet to rule from |
| [`2026-11-25-first-morning-hosts/`](2026-11-25-first-morning-hosts/) | #391 (integrated) | The first morning's new hosts |
| [`2026-11-25-first-run-option-a/`](2026-11-25-first-run-option-a/) | #382 | Option A's first run, written out from install to the first morning |
| [`2026-11-25-forked-conversations/`](2026-11-25-forked-conversations/) | #426 | A conversation forked across computers |
| [`2026-11-25-google-plan-count/`](2026-11-25-google-plan-count/) | #428 | Counting a Google plan without reading it |
| [`2026-11-25-guest-clock-across-sleep/`](2026-11-25-guest-clock-across-sleep/) | #391 (integrated) | The guest's clock across a sleep of the Mac |
| [`2026-11-25-gvproxy-component/`](2026-11-25-gvproxy-component/) | #380 | gvproxy as a part of Silo (25 November 2026) |
| [`2026-11-25-gvproxy-upstream/`](2026-11-25-gvproxy-upstream/) | #430 | gvproxy's changes, ready for upstream (25 and 26 November 2026) |
| [`2026-11-25-integration/`](2026-11-25-integration/) | #375 | All the drafts of 24 November, merged |
| [`2026-11-25-integration-2/`](2026-11-25-integration-2/) | #391 (integrated) | The drafts of 25 November, merged on top of #375 |
| [`2026-11-25-key-or-plan/`](2026-11-25-key-or-plan/) | #391 (integrated) | Claude with an API key or with a plan, per computer |
| [`2026-11-25-lasting-access/`](2026-11-25-lasting-access/) | #423 | Revocations and "lasting access", rescored on #383's and #404's sets |
| [`2026-11-25-ledger-live/`](2026-11-25-ledger-live/) | #440 | The team ledger, built and run for real |
| [`2026-11-25-linux-main/`](2026-11-25-linux-main/) | #381 | Option B ready to decide: a Linux main on the Mac |
| [`2026-11-25-local-models/`](2026-11-25-local-models/) | #384 | A model on your Mac, reached from main through the boundary |
| [`2026-11-25-main-disk-from-outside/`](2026-11-25-main-disk-from-outside/) | #429 | Reading main's disk from outside, when main may be hostile |
| [`2026-11-25-model-prices/`](2026-11-25-model-prices/) | #391 (integrated) | Keeping model prices current for budgets |
| [`2026-11-25-morning-screen/`](2026-11-25-morning-screen/) | #421 | The morning screen, assembled |
| [`2026-11-25-native-builds-through-the-proxy/`](2026-11-25-native-builds-through-the-proxy/) | #391 (integrated) | The native Claude Code build and Codex's WebSocket through the closed network |
| [`2026-11-25-network-link-restart/`](2026-11-25-network-link-restart/) | #391 (integrated) | A computer's network across a restart of gvproxy |
| [`2026-11-25-parser-fixes/`](2026-11-25-parser-fixes/) | #391 (integrated) | The disk, preview and closed-network findings of #401, fixed |
| [`2026-11-25-persistence/`](2026-11-25-persistence/) | #433 | Persistence after a bad night |
| [`2026-11-25-plan-windows/`](2026-11-25-plan-windows/) | #391 (integrated) | A plan's window across devices |
| [`2026-11-25-port-previews/`](2026-11-25-port-previews/) | #384 | Port previews: the wrapper as one component, and the ways out #363 did not try |
| [`2026-11-25-price-channel/`](2026-11-25-price-channel/) | #391 (integrated) | Where the price table comes from, against what Silo sends home |
| [`2026-11-25-put-back-and-chain/`](2026-11-25-put-back-and-chain/) | #436 | Put back with a preview, and the chain without processes |
| [`2026-11-25-reader-model/`](2026-11-25-reader-model/) | #413 | Whose model reads the summary |
| [`2026-11-25-real-night/`](2026-11-25-real-night/) | #377 | A real night, not a scripted one |
| [`2026-11-25-record-format/`](2026-11-25-record-format/) | #386 | One record format, its writer and `silo verify` |
| [`2026-11-25-record-head-off-the-mac/`](2026-11-25-record-head-off-the-mac/) | #391 (integrated) | The record's head off the Mac, and the writer through a power cut |
| [`2026-11-25-red-team-round-two/`](2026-11-25-red-team-round-two/) | #435 | Red team, round two: the components of 25 November |
| [`2026-11-25-second-labeller/`](2026-11-25-second-labeller/) | #391 (integrated) | A second, blind labelling of #383's 1,085 writes |
| [`2026-11-25-secret-redaction/`](2026-11-25-secret-redaction/) | #422 | What the record keeps of secrets |
| [`2026-11-25-shared-host-leaks/`](2026-11-25-shared-host-leaks/) | #432 | The shared host's two leaks, closed and costed |
| [`2026-11-25-shared-linux-host/`](2026-11-25-shared-linux-host/) | #391 (integrated) | One Linux host for a team of five |
| [`2026-11-25-spend-budget-edges/`](2026-11-25-spend-budget-edges/) | #385 | Spend budgets at their edges |
| [`2026-11-25-spend-report/`](2026-11-25-spend-report/) | #427 | The program's own spend report as a budget source |
| [`2026-11-25-ssh-tunnel-tell/`](2026-11-25-ssh-tunnel-tell/) | #391 (integrated) | Telling who drives an SSH session through the boundary, measured |
| [`2026-11-25-summary-injection/`](2026-11-25-summary-injection/) | #391 (integrated) | Keeping the summary reader from being talked round |
| [`2026-11-25-team-budget-in-git/`](2026-11-25-team-budget-in-git/) | #419 | A team's budget kept in its Git repository, measured on GitHub |
| [`2026-11-25-team-ledger/`](2026-11-25-team-ledger/) | #388 | One budget shared by several Macs |
| [`2026-11-25-techniques-from-two-tools/`](2026-11-25-techniques-from-two-tools/) | #378 | Techniques from two open-source tools, and a closed network with no network stack |
| [`2026-11-25-tls-refusals/`](2026-11-25-tls-refusals/) | #425 | TLS refusals, recorded and shown |
| [`2026-11-25-untrusted-parsers/`](2026-11-25-untrusted-parsers/) | #391 (integrated) | The parsers of the drafts, fuzzed and compared |
| [`2026-11-25-uploads-to-seen-hosts/`](2026-11-25-uploads-to-seen-hosts/) | #424 | Uploads to hosts already seen |
| [`2026-11-25-way-in-before-the-relay/`](2026-11-25-way-in-before-the-relay/) | #391 (integrated) | The way into main before the relay (25 November 2026) |
| [`2026-11-25-whole-policy-writes/`](2026-11-25-whole-policy-writes/) | #437 | Grants hidden in whole-policy writes |
| [`2026-11-25-write-log/`](2026-11-25-write-log/) | #391 (integrated) | The write log: libkrun's hook, Silo's side of it, and the checkpoint reader |
| [`2026-11-26-headline-numbers/`](2026-11-26-headline-numbers/) | #439 | The recap's headline numbers, re-run (26 November 2026) |
| [`mac-test-kit/`](mac-test-kit/) | #359 | Silo's Mac test kit |
