# A conversation forked across computers

What the record and a conversation budget must do when a fork carries a conversation: measured on
Claude Code 2.1.332 and Codex 0.172.0, each taking one conversation up in seven ways, against a
stand-in provider that records which computer sent each request.

#347 found that both sides of a fork send the same session ids, and #376 wrote "a conversation
belongs to one computer" into `recognising-a-conversation`. #385 measured three budget rules for a
fork's copy on a scripted client and recommended `carry`. This adds the real programs, the cases
#385 did not run (a fork of a checkpoint, a fork of a fork, a fork that becomes main, the program's
own fork inside one computer), the cache, and the record.

## In short

- **Nothing in the traffic tells main from its fork, so the computer is the only key.** Every
  identifier was the same on both sides in every case: Claude Code's session id (header and
  `metadata.user_id`) and its `device_id`; Codex's `session-id`, `thread-id`,
  `prompt_cache_key`, window id and `installation_id`. A fork with a new machine id
  (`fork-identity`) sent them unchanged too: Claude Code's `device_id` lives in
  `~/.claude.json`, Codex's `installation_id` in `~/.codex/installation_id`, both on the disk the
  fork copies. Codex marks a resumed conversation `history_initialization: cold_resume`, the same
  on main and the fork. Read with the answer as written and no notion of computer, all seven cases
  were one conversation.
- **A fork's copy must be linked at the request it continues, not at the parent's current
  state.** In every case the fork's first request continued, by content, exactly the last request
  the parent had sent before the fork's disk was taken. The link found there gives the right
  starting spend in all seven cases. #385's `carry`, as built, starts the fork at what the parent's
  conversation has counted when the fork first calls: for a fork of a checkpoint that adds every
  turn main made after the checkpoint (here $0.0166 and $0.0207, one turn; it grows with the
  work main did since).
- **A fork of a fork needs the link followed up more than one level, and a fork that becomes main
  needs its own conversations to become main's.** With the link one level up, as #385 built it, a
  fork of a fork finds only what its own parent sent. When a fork replaces main
  (`fork-replacing-main`) and Silo keeps main's conversations as they were, the next request from
  main continues main's old conversation by content (it goes "back" to the turn before the fork)
  and counts main's discarded turns, not the fork's: $0.2028 instead of $0.1696 for Claude Code,
  $0.2138 instead of $0.1717 for Codex.
- **The program's own fork inside one computer stays in its conversation, and Codex names it.**
  `claude --resume <id> --fork-session` sends a new session id, `codex exec fork <id>` a new
  `thread-id` and `prompt_cache_key`; both first requests continue the original by content, so
  the answer keeps them in the conversation, as a request that goes back to an earlier turn.
  Codex also sends `forked_from_thread_id` and `forked_from_ordinal_exclusive` in every request of
  the fork, which joins them even after a compaction; Claude Code sends nothing of the kind.
- **A fork's first request reads main's cache.** The provider keeps a prompt cache per account,
  not per computer, so the fork's first request was 99% cache reads. An estimate that keeps cache
  state per conversation and starts a fork's copy empty prices that request 7 to 9 times too high
  (+662% to +871%). The link has to carry the conversation's cache state with its spend.
- **The record should keep one message store per Mac.** Storing each message once by hash, a
  store per computer held 5.3 times (Claude Code) and 5.6 times (Codex) the bytes of one store
  shared by all computers on the Mac, since each fork re-stores the whole conversation it copied.

## What was run

`run/all.sh` runs `run/scenarios.sh` for each program, then `analyse/analyse.py` on what it
captured. A computer is a folder with a home, a working folder and a machine id, mounted in a
private mount namespace at the same paths for every computer, so a copy of the folder is a cold
fork of the disk (libkrun cannot fork a running computer yet: `forks-of-running-linux`). Each
computer's programs reach the stand-in on their own port: that plays the boundary knowing which
computer sent a request without asking the program.

| Case | Computer | What it does |
|---|---|---|
| A | main | Two turns of one conversation; later two more |
| B | fork | A fork of main after A's two turns takes the conversation up (`--resume`, `exec resume`) for two turns, then main goes on |
| C | forkid | The same with a new machine id |
| D | old | A fork of a checkpoint taken after A's second turn, while main is at its third |
| E | fork2 | A fork of the fork |
| F | main | The program's own fork of the conversation inside main (`--fork-session`, `codex exec fork`) for two turns |
| G | main | The fork's disk becomes main's (`fork-replacing-main`) and the conversation goes on |

The stand-in (`standin/provider.py`) plays Anthropic's Messages API and OpenAI's Responses API: for
each instruction it runs one shell command (Claude Code's `Bash`, Codex's JavaScript `exec`
tool) and then answers, bills each call with stand-in prices, and keeps a prompt cache per
credential. Each program made 24 model requests in 12 steps.

The analysis groups the requests five ways and counts a conversation budget from the stand-in's
bill:

| Way | What it does |
|---|---|
| `across` | The answer before #376: content, then the session join, with no notion of computer |
| `fresh` | One computer per conversation; a fork's copy starts at zero (#373's text, #385's `fresh`) |
| `linked` | One computer per conversation; a fork's first request is looked up among the requests its disk's ancestors sent before the fork, at any depth, and its conversation starts at what the parent's had counted at that request |
| `now` | #385's `carry` as built: the parent's conversations as they stand when the fork first calls, one level up |
| `linked-unsplit` | `linked`, with main keeping its conversations after a fork replaces it |

Results per program: [`results/cc.md`](results/cc.md), [`results/cx.md`](results/cx.md). Each
step's computer, case, session and request numbers are in `results/traffic/*-steps.jsonl`, and
what the stand-in billed in `*-ledger.jsonl`. The captured requests are not committed: they hold
each program's whole system prompt. To run again, with `claude` and `codex` on the path (Codex
from `npm i @openai/codex`), as root for the mount namespaces:

```
CODEX=$(command -v codex) run/all.sh
```

## What the conversation budget counted

Claude Code, dollars on the conversation at the end of each step:

| Step | Computer | Case | Step's cost | across | fresh | linked | now | linked-unsplit |
|---|---|---|---|---|---|---|---|---|
| 2 | main | A | 0.0167 | 0.1188 | 0.1188 | 0.1188 | 0.1188 | 0.1188 |
| 4 | fork | B | 0.0170 | 0.1527 | 0.0339 | 0.1527 | 0.1527 | 0.1527 |
| 5 | main | A | 0.0166 | 0.1693 | 0.1354 | 0.1354 | 0.1354 | 0.1354 |
| 7 | old | D | 0.0166 | 0.2029 | 0.0166 | 0.1354 | 0.1520 | 0.1354 |
| 8 | fork2 | E | 0.0171 | 0.2201 | 0.0171 | 0.1699 | 0.1699 | 0.1699 |
| 12 | main after G | G | 0.0168 | 0.2875 | 0.0168 | 0.1696 | 0.1696 | 0.2028 |

`linked` gives each copy exactly its own history's spend: the fork of the checkpoint (D) starts at
main's $0.1188 at the checkpoint and adds its own turn; the fork of the fork (E) carries main's two
turns and the fork's two. `fresh` lets every copy spend the budget again; `across` stops every
copy when any of them reaches it. Codex's numbers differ, the rows agree (`results/cx.md`).

## What it changes in the plan

- **`recognising-a-conversation`** (notes): the identifiers measured; the link a fork's
  conversation needs (at the request continued, among requests sent before the fork, at any
  depth), the cache state it carries, and what a fork that becomes main does. Codex's
  `forked_from_thread_id` as one more way to join.
- **`what-a-conversation-budget-promises`** (notes): #385's recommended option, "starts at what
  its parent's had spent", made exact: at the request the fork continues, not when the fork first
  calls. The checkpoint case is where they differ.
- **`fork-replacing-main`** (notes): when a fork becomes main, its conversations become main's,
  with their spend; main's own go to the old main kept aside.
- **`store-messages-once`** (notes): one store for every computer on the Mac.

## Not verified

- The provider is a stand-in. Its cache follows the providers' pages (per account or
  organisation, by prefix; OpenAI's `prompt_cache_key` only routes) but no real provider was
  called: there is no provider key in this container.
- Forks are cold: a copy of the disk, the program started again. A live fork (memory snapshot)
  would copy a program in the middle of a call; what each side sends then was not measured.
- Short conversations only, with no compaction and no extended thinking. Whether Claude Code's
  `--resume` rewrites earlier turns after a compaction or with thinking blocks, which would break
  the content link, was not tested here (#371 found it drops a leading `cd <dir> &&` from earlier
  tool inputs).
- Gemini CLI was not run.
- The analysis replays the captured traffic with prices from the stand-in's bill; no boundary
  enforced the budgets live. #385's prototype is where `carry` runs live; the changes above are
  not built into it.
