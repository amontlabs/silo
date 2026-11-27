# All the drafts of 24 November, merged

For polarzero, 25 November 2026, by an agent. This branch is `main` (`1b8f191`) with every open
draft pull request merged into it, in the order #364 proposes, with #370 to #374 added after.
`plan check` passed after each merge. Nothing here is new research: the only changes made on this
branch are this page, one folder rename, one note and one section removed.

Two ways to use it:

- Merge the original pull requests one by one, in the order below, and make the fix-up listed for
  each by hand.
- Merge this pull request instead. Every source branch below is an ancestor of this one, at the
  head listed, so it holds every commit of every draft.

Left out: #357 (not a draft; a fix to the home page's film test) and the Dependabot updates #187
and #250.

## The order, and the fix-up each draft needed

Heads are those of 25 November, 10:30 UTC. A draft pushed to since then needs merging again.

| Step | PR | Head | Fix-up on merge |
|---|---|---|---|
| 1 | #350 | `d06f7ad` | none |
| 1 | #351 | `8c5d395` | none |
| 1 | #352 | `b24b1ff` | rename `research/first-build-decisions/` to `research/2026-11-24-first-build-decisions/`, the name #354 and #359 link to. A commit of its own on this branch |
| 1 | #354 | `224b2c3` | none |
| 1 | #355 | `ddb21a2` | none |
| 1 | #358 | `4fe868e` | none |
| 1 | #359 | `3c72201` | none |
| 1 | #363 | `fcf5a96` | none |
| 1 | #365 | `2f603cd` | none |
| 1 | #366 | `fd3b30b` | `network-stack.md` and `checkpoint-per-system.md` clash with #363 and #365: each side adds a section at the end. Both kept, #363's and #365's first |
| 1 | #364 | `f255994` | none |
| 2 | #348 | `b45be1c` | none. It rewrites `order.json`; the later drafts place their ids in its new layout |
| 3 | #347 | `3665289` | five notes files clash, each side adding sections at the end: `fork-shares-disk-blocks.md`, `network-stack.md`, `checkpoint-per-system.md`, `ending-the-encrypted-connection.md`, `forwarded-byte-for-byte.md`. All sections kept, those already merged first |
| 3 | #353 | `f44511f` | none |
| 3 | #356 | `210f89a` | `secret-placeholder.md` clashes with #347: both sides add sections. All kept, #347's first |
| 4 | #368 | `bb0fced` | none |
| 4 | #369 | `28ba06b` | `order.json`: keep #348's layout of the stage "The app", and put `own-processes-in-a-denial` after `denial-per-program`. `vendor-runs-the-model-calls` merges by itself after `program-that-cannot-be-read`. Eight notes files clash with drafts merged before it, by added sections (`fork-and-sign-in`, `logins`, `hold-limit-length`, `how-a-hold-ends`, `budget-denies-or-holds`, `denial-per-program`, `denied-tool-call-mechanism`, `reading-each-agent-program`): all kept, the merged ones first |
| 4 | #367 | `62f9f66` | nothing to merge: its one commit is in #369, which is based on it. #367 can be closed |
| 5 | #360 | `e12a714` | `order.json`: keep #348's layout, then `what-silo-sends-home` merges by itself after `nothing-to-set-up`. Drop #360's `plan/computers/decisions/simulators-for-parallel-agents.{json,md}`: #361 adds the same id, and two entries with one id fail `plan check` |
| 5 | #361 | `d3d9caf` | `order.json`: keep #348's layout; `simulators-for-parallel-agents` after `silo-ships-recipes`, `xcode-in-a-computer` right after it; `signing-keys-re-signed` and `apple-keys-in-a-computer` after #356's three keys, all after `secret-placeholder`; `builds-on-your-iphone` merges by itself after `asks-are-native`. Five notes files clash by added sections: all kept. Then #360's case for the simulator becomes a note on #361's entry (below) |
| 5 | #362 | `04b5efa` | `order.json`: keep #348's layout; `record-edits-show` after `record-file-format` and `person-in-a-control-fact` after `who-made-each-change`, both in #348's first-build block. The fourteen team entries merge by themselves after `forwarding-tool` |
| 6 | #370 | `b0a5eef` | `network-stack.md`, `restore-saves-first.md`, `thinning-checkpoints.md`: added sections, all kept |
| 6 | #371 | `8c8b0cf` | `recognising-a-conversation.md`: added sections, all kept |
| 6 | #372 | `3c155ed` | none |
| 6 | #373 | `95d74f7` | `main-on-macos-first.md`, `network-stack.md`: added sections, all kept |
| 6 | #374 | `803783a` | `budget-denies-or-holds.md`, `program-that-cannot-be-read.md`: added sections, all kept |

Every clash in a notes file was two drafts each adding sections at the end of the same file. In
each, neither side removed a line the other changed, so keeping both is the whole fix. None of #370
to #374 touches `order.json`.

## The simulator entry

#360 and #361 each add `simulators-for-parallel-agents`, with opposite recommendations: #360 lets
the agents clash and has the record show who erased which simulator, and #361 gives each agent its
own simulator up to a maximum. #364 proposed keeping #361's with #360's record added. On this
branch, #361's entry is unchanged, and #360's record and its two facts (Claude Code reuses a running
simulator; a reported issue where it ignores the requested one) are a dated note at the end of
`plan/recipes/decisions/simulators-for-parallel-agents.md`. The answer was not edited.

## A comparison moved out of the public plan

#347 adds a section "What other sandboxes do with a secret" to the notes of `secret-placeholder`.
It compares Silo with other products and ends on why one would pick Silo. polarzero's rule keeps
that out of this repository, so this branch removes the section, and the text is kept in the
private research repository. #347's own branch still has it; merging #347 by itself needs the same
removal.

## What changed since #364 was written

#364 found that the research drafts conflict with nothing. Their heads have moved since: #363, #365
and #366 now add notes to the same entries, and so do #347 and the drafts after it. All of these
clashes are added sections, as in the table.

## Not done here

- The full check (`tools/ci/check.sh`) was not run; it runs at merge. Relative links in every
  Markdown file this branch changes were checked offline, and none is broken.
- Some notes files from the source drafts end with a blank line, and some captured outputs in
  #353 keep their trailing spaces. They are left as the drafts have them.
- #359's folder is `research/mac-test-kit/`, without a date, unlike the others. It was not renamed,
  since #361 links to it.
- No position from #364's sections 1 to 17 was written into the plan.
