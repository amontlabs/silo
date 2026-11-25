# The challenged decisions, each ready to rule

For polarzero, 25 November 2026, by an agent. The research drafts of 24 November challenge about
forty decisions of the plan. This pull request writes each challenge into the plan, so that every
one can be taken or dropped from the Workshop in one step once it is merged. It writes no verdict
and changes no list. Every decision in force stays in force until polarzero takes its replacement.

`plan show <id>` prints any entry named here, with its notes.

## How each challenge is written

The challenges fall into three kinds, by the state of the decision they challenge.

- **An open decision** (no verdict yet, 20 of them). Its answer now holds the replacement, which
  is what the Workshop recommends. Its notes open with a new `## The choice`: the case, the
  options, and the earlier answer as one of them. The earlier choice is kept below it, under
  `## The earlier choice`. Taking the entry takes the replacement; dropping it says no to it.
- **A decision polarzero took** (11 of them). A new decision names it in `replaces`, sits next to
  it in `order.json`, and has its own `## The choice`. The decision in force gets a short note,
  "Challenged", that points to its replacement. Two of these replace decisions agents took
  (`core-simulator`, `fuzz-untrusted-parsers`): agents may take those with a judge, as
  `owners-part-on-engine` allows, and polarzero confirms them or not.
- **A challenge that changes no answer** (5 of them). It is a note on the decision: the answer
  stays, and the note says what to do instead and where.

One decision is new and not a replacement: `foreign-credential-refused`, which the challenge to
`ending-the-encrypted-connection` needs.

## Every challenge, and where it now is

| Decision challenged | State | Challenged in | Now | Recommended |
|---|---|---|---|---|
| `hold-limit-length` | open | #350, #355, #367, #369 | answer edited | a length per client; a held model reply kept alive |
| `hold-sentence` | open | #350, #355 | answer edited | the ask outlives its hold; a late yes passes the exact request once |
| `how-a-hold-ends` | open | #355, #367, #369 | answer edited | 400 in the provider's format; a push refused in Git's terms |
| `adapters-follow-providers` | taken | #367, #369 | `adapters-follow-formats` | readers follow formats, chosen by path |
| `secret-placeholder` | open | #347, #355 | answer edited | swap only in the secret's places; a placeholder elsewhere is forwarded and recorded |
| `secret-values` | open | #355 | answer edited | name the forms removed from replies |
| `limits-on-a-secret` | open feature | #355 | text edited | "requests that read", and named places |
| `ending-the-encrypted-connection` | open | #355 | answer edited, with `foreign-credential-refused` | a rule can ask to read a host; each request names its handshake's host |
| `what-silo-changes-in-traffic` | open | #355 | answer and question edited | three ways only, named |
| `forwarded-byte-for-byte` | open | #355 | answer edited | refuse a request whose framing reads two ways |
| `content-kept-of-what-was-sent` | open | #355 | answer and property edited | method, host and path of what Silo reads |
| `what-a-receipt-holds` | open | #355, #371 | answer edited | the test runner's own exit code, and what weakens checks |
| `store-messages-once` | taken | #355 | `message-identity-keyed` | a keyed hash for every message |
| `where-precautions-live` | taken | #351 | `what-a-computer-sees-of-opinions` | a computer sees the descriptive opinions, never precautions |
| `the-chain` | taken | #371 | note, and one row of its agent's table | match a tool call to processes by command text, then descent |
| `what-a-fact-carries` | taken by agents | #352 | note | the answer stays; the addition goes in `kinds-of-lines` and `sources` |
| `whole-computer-grain` | taken | #352 | note | the answer stays; a Mac test with `claude --resume` is added |
| `how-spend-is-shown` | open | #350, #355, #374 | answer edited | a subscription's window first, dollars beside it |
| `budget-rule-or-precaution` | taken | #355, #374 | `budgets-in-dollars-or-a-window` | a reserve on the window is a rule, a share per computer a precaution |
| `word-budget` | open | #355 | answer and word edited | dollars or a share of a plan's window |
| `recognising-a-conversation` | open | #355, #371, #347, #373 | answer edited, sets the word `conversation` | a fallback model stays in its conversation |
| `model-of-access-first` | taken | #348 | `model-of-access-on-real-grants` | written for the grants of the first build that has any |
| `app-as-text` | open feature | #348 | text and `needs` edited | a view can reach the command line before the app |
| `network-stack` | open | #348, #352, #358, #363, #364, #366, #370, #373 | answer edited | Silo's stack from main's first start, gvproxy only sealed (if option A) |
| `idle-memory` | taken | #352 | `idle-memory-by-kind` | the property for Linux computers; a macOS computer's memory a size you choose |
| `where-it-runs` | taken | #352 | `where-computers-run` | a macOS computer in a cloud only on a whole leased Mac |
| `sizing-target` | open | #361, #350 | answer edited | booted simulators as a load of their own |
| `agents-drive-the-screen` | taken | #352 | note | the answer stays; drive the screen from outside, through the framework |
| `what-an-agent-inside-may-do` | taken | #352 | `agents-may-do-and-who-answers` | you answer in the app or with Touch ID, never by a command |
| `core-simulator` | taken by agents | #352 | `core-and-its-loop-simulated` | the core and its loop; liveness once faults stop |
| `engine-client` | taken by agents | #352 | note | the answer stays; drop "ids with a generation" from its notes |
| `fuzz-untrusted-parsers` | taken by agents | #352 | `parsers-fuzzed-and-compared` | reject input another reader could read differently |
| `checkpoint-per-system` | open | #365, #352, #366 | answer edited | from Silo's list of blocks written; a clone where Silo does not see the writes |
| `thinning-checkpoints` | open | #365, #351, #352, #354, #370 | answer edited | 24 hours whole; three kinds kept whatever their age; the disk sets the reach |
| `fork-from-the-moment` | taken | #354, #348 | `going-back-on-a-shared-main` | put back the paths you name, or fork |
| `fork-identity` | open | #355 | answer edited | a remote database through the fork's own secret |

The word `conversation` ("a sequence of turns with one model") is challenged with
`recognising-a-conversation`, which now sets it again.

## Where the drafts disagree, and what is recommended

Most positions follow #364, which read all twenty drafts together. Where #364 left the choice to
polarzero, or a later draft disagrees with it, the entry's choice gives every option and this
recommendation:

- **How long a hold lasts.** #350 keeps 90 seconds, #355 wants 10 minutes, #353 a length below each
  client's timeout. Recommended: #364's synthesis, a length per client, and a held model reply kept
  alive by an event its format documents (#369), since then no program is the limit.
- **How long an unanswered ask stays.** #355 closes it after 30 minutes; #354 and #364 keep it until
  you answer or set it aside. Recommended: until you answer, so the morning shows what the night
  held.
- **What "the same request" is.** #347 and #350 match on method and path; #353 and #355 add the body.
  Recommended: the body too, or the client's own request id. A yes to a charge of 40.00 let a charge
  of 9,900.00 through under the looser match (#355).
- **The status of a refusal.** #347 proposed 451 and 402 per program, #351 a 429, #367 found 403
  retried, #369 found 400 in the provider's format stops all eight programs. Recommended: #369's.
- **A placeholder seen on another host.** #356 keeps "denied"; #361 refuses a placeholder-signed
  request to another host; #347 and #355 forward and record. Recommended: forward and record.
- **What Silo changes in traffic.** #355 (page 1) proposes three ways, #355 (page 5) two, #364 three
  others. Recommended: one wording with three named ways that holds all five cases.
- **A credential Silo did not give.** #351 refuses it on every host Silo reads; #355 only on a host
  that gets a secret; #353 refuses neither. Recommended: #355's scope, as #364 says, with the agent
  program's own sign-in noted.
- **Main's network in the first build.** #352, #348, #364 and #370 start main on Apple's NAT until
  the sealed gvproxy passes Mac test 07; #373 needs Silo's stack from the first start. Recommended:
  #373's, since it recommends option A; the entry says which applies if polarzero takes option B.
- **How long dailies are kept.** The plan says 30 days, #370's spec 7. Recommended: 30 days as the
  reach, with the disk limit, the Timeline and an alert deciding what each Mac really keeps (#365).
- **A message's hash.** #355 keys the hash only for a message with a marker. Recommended: every
  message, one way of naming them.

## For polarzero to rule

The 31 entries that change an answer, and `foreign-credential-refused`, are open in the Workshop
once this pull request is merged.
Three of them change a statement in force, so only polarzero can take them:

- `word-budget` sets the word `budget` again.
- `recognising-a-conversation` sets the word `conversation` again.
- `idle-memory-by-kind` sets the property `idle-computer-gives-memory-back` again, for Linux
  computers only.

Two depend on the first build: `network-stack` (option A or B) and `model-of-access-on-real-grants`.

## What this does not do

- It does not merge the drafts it rests on. Many of the entries it edits also get notes from those
  drafts (#347, #356, #361, #363, #365, #366, #370, #371, #373, #374). This pull request puts its
  changes at the top of each notes file and the drafts add theirs at the bottom, but where both edit
  the same answer, #356's edit of `secret-placeholder`, the merge needs a hand: keep this answer, and
  drop #356's added sentence, as #364 says.
- It does not rewrite #355's other proposed entries (`credential-minting-held`, `secret-signers`,
  `database-secret`, `budget-in-a-window` and others), which are new questions, not challenges.
  They are named in the choices where an answer leans on them.
- It does not check any number again: each comes from the draft named beside it. What a draft
  marked as not verified stays so.
- The record's format (`the-chain`, `what-a-fact-carries`) is being reconciled by another draft;
  the two edits here are the row of `the-chain`'s table that #371 measured and the line of the word
  `conversation`.
- The Mac test for `whole-computer-grain` (`claude --resume` on a checkpoint taken mid-write) is not
  in #359's kit yet.

`plan check` passes on this branch.
