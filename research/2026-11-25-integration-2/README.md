# The drafts of 25 November, merged on top of #375

For polarzero, 25 November 2026, by an agent. This branch is #375's branch (`b567132`, every draft
up to #374 merged into `main` at `1b8f191`) with every draft from #376 to #389 merged into it.
`plan check` passed after each merge. Nothing here is new research. The only changes made on this
branch are this page, one heading, notes on the entries where two drafts disagree, three notes or
`needs` that drafts asked for once their base landed, and the merges themselves.

Two ways to use it:

- Merge #375's drafts as #375's README says, then the drafts below in this order, with the fix-up
  listed for each.
- Merge this pull request instead. Every source branch, #375's included, is an ancestor of this
  one at the head listed, so it holds every commit of every draft.

## The order, and the fix-up each draft needed

Heads are those of 25 November, 11:10 UTC. A draft pushed to since then needs merging again.
Stacked drafts come after their base: #379 and #383 on #375, #385 on #374, #387 on #377 on #371,
#388 on #362.

| PR | Head | Fix-up on merge |
|---|---|---|
| #376 | `459e9b0` | `order.json`: keep #348's layout, then place #376's twelve new ids each after the id it follows in #376 (`agents-may-do-and-who-answers`, `where-computers-run`, `core-and-its-loop-simulated`, `model-of-access-on-real-grants`, `idle-memory-by-kind`, `message-identity-keyed`, `going-back-on-a-shared-main`, `what-a-computer-sees-of-opinions`, `foreign-credential-refused`, `adapters-follow-formats`, `budgets-in-dollars-or-a-window`, `parsers-fuzzed-and-compared`). `secret-placeholder.json`: both sides edited the open answer. #376's answer kept, with #356's two last sentences added ("A secret is bound to the hosts you allowed for it. Whether it can also be bound to a method and a path on such a host is to find out."). `the-chain.md`: #371's note and #376's section say the same challenge; both kept, #371's first |
| #377 | `41ca9dd` | none |
| #378 | `08e74ee` | six notes files, each side adding sections at the end: `network-stack`, `fork-identity`, `budget-as-a-rate`, `how-spend-is-shown`, `program-that-cannot-be-read`, `what-a-budget-promises`. All kept, the merged ones first |
| #379 | `906846f` | none |
| #380 | `df198ec` | `network-stack.md`: added sections, all kept |
| #381 | `3f0dcd6` | seven notes files by added sections (`computer-image`, `main-on-linux`, `main-on-macos-first`, `linux-vm-engine`, `checkpoint-per-system`, `file-changes-from-outside`, `processes-from-inside-linux`), all kept. Then `computers-linux-main-on-a-mac` needs `mac-silo-needs` and `network-before-rules`, which #381 asks for "on #348's tree" |
| #382 | `1e5c169` | four notes files by added sections (`reading-each-agent-program`, `agent-summary-beside-receipt`, `network-place`, `what-a-receipt-holds`), all kept |
| #383 | `6afc7cf` | none |
| #384 | `4b6cf69` | `ports-reached-from-your-mac.md`, `program-that-cannot-be-read.md`: added sections, all kept |
| #385 | `f1f3a32` | `program-that-cannot-be-read.md`, `what-a-budget-promises.md`: added sections, all kept |
| #386 | `9861826` | five notes files by added sections (`kinds-of-lines`, `store-messages-once`, `the-chain`, `what-a-fact-carries`, `who-made-each-change`), all kept. Then #386's note for `record-edits-show`, which it kept in its README because #362 had not landed, added to that entry |
| #387 | `b0fc2b4` | `agent-summary-beside-receipt.md`: #387's paragraph has no heading, so merged after #382 it fell under #382's heading. It gets one of its own, "The catch rate on new claims" |
| #388 | `31302e3` | `what-a-budget-promises.md`: added sections, all kept |
| #389 | `e2c98b0` | `low-disk`, `sizing-target`, `thinning-checkpoints`: added sections, all kept. Then #389's note for `simulators-for-parallel-agents`, which it kept in its README because #361 had not landed, added to that entry |

Every other clash was two drafts each adding sections at the end of one notes file. In each,
neither side removed a line the other changed, so keeping both is the whole fix. Only #376 touches
`order.json`.

## Where the drafts disagree, and what is recommended

Each row is also a note on the entry named, under "Where the drafts of 25 November disagree
(integration)", so the Workshop shows it beside the choice. No answer, verdict or list was changed.

| Drafts | They disagree on | Recommended | Note on |
|---|---|---|---|
| #370 against #378 and #380 | A closed main: "not possible" without a patched gvproxy (#370, step 2), or possible now with a card that leads nowhere and a proxy over vsock (#378) | #378's card for the first build, closed or limited to a list; #370's step 2 table says so; test 07 stops blocking option A | `network-stack`, `network-before-rules` |
| #363 against #380 | Whether `-sealed` closes the Mac and the local network. gvproxy 0.9.0 forwards ping, which `-sealed` does not filter | #380's patch with `-outbound-deny` replaces #363's; test 07 adds a ping | `network-stack`, `network-before-rules` |
| #382 against `network-stack` (#376's answer) | Two minutes of Apple's NAT at main's first start, to install the vsock relay, against "never Apple's NAT" | Apple's NAT for the first start until test 07 runs on a Mac; then #380's `-closed` gvproxy, which #380 measured on Linux: nothing out, SSH in | `way-in-before-the-relay` |
| #374, #378, #385 (and #388) | Calls under way at the limit: they finish (#374, 35% to 56% over), one at a time near the limit (#378, not measured), or hold each call's typical output and cut what passes anyway (#385, 7% under, never over) | #385's fourth option | `what-a-budget-promises` |
| #376 (and #373) against #385 | A conversation continued in a fork starts at zero, so a $0.50 budget spent $1.10 across main and a fork | The fork's copy starts at the parent's spend; held to $0.83 | `what-a-conversation-budget-promises`, `recognising-a-conversation` |
| #376 against #386 | Two replacements of the taken `store-messages-once`: a keyed hash (#376), a request that names the one it extends and lists what it added (#386) | One replacement holding both; its text is in the note | `message-identity-keyed`, `store-messages-once` |
| #378 against #385 | Batches and background work under a budget: refused until counted (#378), or held at their bound and counted (#385) | #385, since it counts them to the cent; #378's refusal stays for work still not counted | `budget-and-work-out-of-sight` |
| #377 against #387 | Whether the receipt's own warnings matter more than the claim checker. #387: they fire on 5 of 16 true claims | Show the warnings as facts, and check the words with #387's model reader | `agent-summary-checked` |
| #362 against #388 | A pooled team budget needs a round trip per call (#362), or leases every dozen calls (#388, within 0.4%) | #388's leases | `team-budget` |

## #382's gaps, after the later drafts

| Gap | State on this branch |
|---|---|
| 1, the first SSH before the relay | Open: `way-in-before-the-relay`. #380 measured its gvproxy option on Linux |
| 2, import out of option A | Open, for polarzero (below) |
| 3, open or closed in A | Closed is possible with #378's card |
| 4, twenty first-seen host alerts | Open, note on `alert-only-on-a-change` |
| 5, guest clock after sleep | Open; #378's fix is now a note on `mac-kept-awake` |
| 6, Codex's commands from traffic | Open, note on `reading-each-agent-program` |
| 7, fewer catches without processes | Measured again by #387: about a fifth fewer |
| 8, a false «All tests pass.» matched | Fixed by #387. #382's repro exits 0 on this branch and prints WRONG on #377's tree |
| 9, the only way back costs the other agent's night | Open, for polarzero (below) |
| 10, a program that ignores the proxy | Open, note on `network-place` |
| 11, Claude Code's install-script build through the proxy | Not run anywhere yet |
| 12, `codex` named on the first screen | Open, a line in #373's `specs.md` |

## For polarzero to rule

Collected from the drafts, with what this branch recommends where two disagree.

- `network-stack` and `way-in-before-the-relay`: #378's card for the first build; Apple's NAT only
  for the first start until test 07 runs.
- `what-a-budget-promises`: hold the typical output and cut what passes anyway (#385).
- `what-a-conversation-budget-promises`: a fork's copy starts at the parent's spend (#385).
- `budget-and-work-out-of-sight` (#385, new): hold the bound until the result passes Silo.
- `store-messages-once`: one replacement, keyed hash and a list of what each request added.
- `record-damaged-at-start` (#386, new), `who-made-each-change` (`device`, #370 and #386), and
  where a copy of the record's head goes (`record-edits-show`, #386).
- `agent-summary-checked` (#387, new): a model reader run as an extension.
- `pooled-budget-ledger` (#388, new): leases from one ledger.
- `ssh-keys`, `exchanged-tokens`, `cloud-credentials` (#379).
- `outcome-of-a-call`: widen "exposed" to access given to someone (#383).
- Which main first, and `checkpoint-per-system`'s first sentence (#381).
- `thinning-checkpoints` and `low-disk` (#389).
- Keep import, `silo look` and `silo put-back` in option A (#382).
- Every entry #376 wrote: about forty, each takes or drops in one step.

## Not done here

- The full check (`tools/ci/check.sh`) was not run; it runs at merge.
- No draft's research was run again, except #382's repro on this branch and on #377's tree.
- #389 was opened at 11:09 UTC, as this branch was built. Drafts opened later are not in.
