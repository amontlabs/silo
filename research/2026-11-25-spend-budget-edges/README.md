# Spend budgets at their edges

Research of 25 November 2026, by an agent, for polarzero. It extends the boundary of #374 to the
routes around a budget that #351 lists and #374 left out: OpenAI's Responses API with state kept
at the provider, Chat Completions without usage, searches billed per call, prompt caching after a
pause, batches, background responses, Gemini's cache billed by the hour, a budget reached in the
middle of an answer, and a conversation taken up by a fork. Each route was run first on #374's
boundary as it is, to show where its count fails, then fixed and run again against stand-in
providers that bill every call.

Nothing here is a decision. One new open decision and notes on four entries carry the choices;
they are listed at the end.

## In short

- **Four kinds of spend went by at $0.** Background responses ($1.06 billed in the run), batches
  ($4.47), Gemini's cache storage ($5.52) and the searches a provider runs (83% of a run's cost)
  were not counted at all. All four are now counted at the bill, to the cent for all but cache
  storage, which is within 0.2%.
- **Work that runs after the call is held at its bound.** A batch, a background response or a cache
  leaves only if its whole bound fits, and holds it until its result passes Silo. A batch of 400
  requests under a $2 budget was refused with "this batch of 400 requests may cost up to $8.82,
  which it holds until its results are read, and this computer has $1.81 left of $2.00". Open
  choice: `budget-and-work-out-of-sight`.
- **A chained Responses call was estimated 99.9% short.** A request that names
  `previous_response_id` or a `conversation` sends only its new input; the provider bills the whole
  chain. Estimated from the counts of the response it names, the input was exact.
- **Cutting calls at the budget holds it to 1% over, and pays for answers nobody uses.** Claude Code, 8
  subagents, $3: running calls finish ended at $3.71 to $4.05; cut at the budget, $2.99 to $3.03,
  with $0.52 to $0.92 a run paid for answers nobody used. Claude Code retried each cut call at once,
  got the 400 and printed Silo's sentence.
- **Holding the typical output, with a cut behind it, is the "never over" answer.** It stopped 7%
  under the budget in every Claude Code run, and held one call that reasoned 60,000 tokens to the
  budget ($1.00 of $1) where the typical hold alone ended at $1.52.
- **The input check missed a cache that had expired.** Priced as cache reads, the input of a call
  after a pause was 24 times short. The boundary now keeps each conversation's cache lifetime and
  prices an expired cache as a write again.
- **A fork can spend a conversation's budget again.** With a $0.50 conversation budget, #373's
  text (a fork's conversations start at zero) let one conversation spend $1.10 across main and a
  fork. A fork's copy that starts at what the parent's had spent held it to $0.83, and main's own
  copy went on. The computer's budget held every turn of both.
- **A Chat Completions stream that does not ask for usage cannot be counted.** Its visible text
  missed 93% of its cost. Under a budget it is now refused with a sentence that says what to set.

## What was built

All in #374's prototype, `research/2026-11-24-spend-budgets/`, which this PR changes:

| File | What changed |
|---|---|
| `boundary/src/outside.zig` (new) | Tells apart the requests that start, follow or end work at the provider: OpenAI's batch file upload, `/v1/batches`, the batch object, its results file, `GET` and `cancel` of a response; Anthropic's `/v1/messages/batches` and its results; Gemini's `cachedContents` create, PATCH and DELETE. Computes a batch's bound from its requests, and its cost from its results, at the batch price. |
| `boundary/src/usage.zig` | A response's id and status, so a `queued` background response is not read as a finished call with no cost. `web_search_call` items and Gemini's grounding queries, as searches. |
| `boundary/src/spend.zig` | Reads `previous_response_id`, `conversation`, `background`, `n` and `stream_options.include_usage` from a request. The error event that ends a cut stream, in each provider's shape. New settings, below. |
| `boundary/src/budget.zig` | Admission of a whole bound, for work out of sight. A ledger that starts at another's spend, for a fork's copy of a conversation. A conversation total. |
| `boundary/src/main.zig` | Holds out-of-sight work until its result passes, and settles it then; a watchdog that settles a cache when its lifetime ends and cuts calls under way when a total is passed; a ledger per conversation; a fork's spend in its parent's ledger; the stored context of a response or conversation by its id; each conversation's cache lifetime. |
| `prices.json` | Batch rows at half price, OpenAI's $10 and Google's $14 per 1,000 searches, Gemini's storage per token-hour. |
| `fake/provider.py` | The stand-ins keep responses and conversations, run background responses and batches, keep a Gemini cache by the hour, search, answer `n` choices, and bill each in `ledger.jsonl`. Their cache lifetimes can be scaled so a 5-minute cache passes in 3 seconds. |
| `run/edges.py`, `run/edges.sh`, `run/edges_check.py` | One scripted case per route, run under its config, and checked: what the boundary counted against what the stand-in billed, and what the case must show. |

The new settings of the boundary's config, each off unless set, so that #374's runs are unchanged:

| Setting | What it does |
|---|---|
| `out_of_sight: "hold"` | Background responses, batches and caches are admitted only if their whole bound fits, and hold it until their result passes. |
| `chat_without_usage: "deny"` | Under a budget, a Chat Completions stream that does not ask for usage is refused. |
| `mid_stream: "cut"`, `cut_share` | Once what is counted passes `cut_share` of a total, each call under way it covers is ended, with an error event in the provider's shape. |
| `cache_ttl_aware` | With `input_price: "likely"`, a cache whose lifetime has passed is priced as a write. |
| `fork_conversations` | `fresh`, `carry` or `shared`: what a fork's copy of a parent's conversation starts from. |
| `conversation_total` in a computer's limits | A total for each conversation on that computer. |
| `fork_of` on a computer | It is a fork of that computer, and with no limits of its own spends from its parent's budget. |

## How it was run

As in #374: a private network and mount namespace per run, stand-in providers behind the boundary,
Silo's test authority only in each program's own environment. The edge cases are scripted
(`run/edges.py`), since no agent program here makes batches or background responses on demand.
The mid-stream runs use Claude Code 2.1.332 with 8 subagents, as #374's overshoot runs did.
Codex and Gemini CLI were not installed in this container.

```
run/tls.sh
(cd boundary && zig build && zig build test)
BOUNDARY=<#374's build> run/edges.sh before     # each case on #374's boundary
run/edges.sh after                               # each case on this one
ONLY="finish-time reserve-typical cut typical-cut" run/overshoot.sh 3 claude-opus-5-5 1 2 3
python3 run/table.py claude-opus-5-5 3
```

The outputs are in [`results/`](results/): [`before-374.txt`](results/before-374.txt),
[`after.txt`](results/after.txt) and
[`overshoot-claude-code.md`](results/overshoot-claude-code.md).

## Results

### Each route, before and after

The stand-in's bill against what the boundary counted, per run. "Before" is #374's boundary;
"after" is this one.

| Route | Billed | Counted before | Counted after | What else the run shows after |
|---|---:|---:|---:|---|
| Responses with `previous_response_id` and `conversation` | $0.40 | $0.40 | $0.40 | The input of 5 chained calls, estimated before each left: 99.9% short before, exact after |
| Background responses, $1.50 budget | $1.06 before, $0.65 after | $0 | $0.65 | Two were refused once held bounds filled the budget; one cancelled was counted from the cancel's reply |
| Web search (Responses) and grounding (Gemini) | $0.10 | $0.02 | $0.10 | 7 searches counted |
| Chat Completions, streams without usage refused | $0.41 before, $0.25 after | $0.26 | $0.25 | The 2 streams without `include_usage` refused, the 3 others passed |
| Chat Completions, streams without usage estimated by time | $0.41 | $0.26 | $0.41 | The 2 estimated: $0.16 for $0.16 billed, with the stand-in's own speed |
| Anthropic cache, then a pause past its lifetime | $1.01 | $1.01 | $1.01 | Input priced before each call, against its bill: 24 times short after each pause before, at or above it after |
| Gemini cache for a day, then one for 30 minutes, extended and deleted | $5.52 before, $0.12 after | $0 | $0.12 | The day of storage refused at $6.48 under $1 |
| OpenAI and Anthropic batches, $2 budget | $4.47 before, $0.58 after | $0 | $0.39 | The batch of 400 refused; the one whose results were never read stays held at $0.28 ($0.20 billed) |

### A budget reached mid-stream

Claude Code, Opus 5.5, 8 subagents, a $3 budget, three runs each
([`results/overshoot-claude-code.md`](results/overshoot-claude-code.md)). The first two rows
repeat #374's numbers on this build, run for run.

| How a call is admitted | Spent, each run | Over the budget, worst | Paid for answers that were cut |
|---|---|---|---|
| Running calls finish, output counted by time | $3.71, $4.05, $3.92 | +35% | none |
| Each running call holds its typical output | $2.74, $2.78, $2.80 | 7% under | none |
| Cut at the budget | $3.02, $3.03, $2.99 | +1% | $0.92, $0.52, $0.55 |
| Typical hold, and cut at the budget | $2.75, $2.78, $2.80 | 7% under | none |

What Claude Code did when a call was cut: it retried it within 40 ms, got the 400 refusal, and
ended with "API Error: 400 Silo: this computer's budget of $3.00 is reached ($3.04 spent)". The
error event of the cut itself was not shown, since the retry replaced it.

One long call, scripted: four calls bring a $1 budget to $0.32, then one reasons for 60,000
tokens.

| How a call is admitted | Spent | The long call |
|---|---:|---|
| Running calls finish | $1.52 | whole |
| Typical hold | $1.52 | admitted, since the typical output fits; whole |
| Cut at the budget | $1.00 | stopped at 17 s, with Silo's sentence in Anthropic's error event |
| Typical hold, and cut | $1.00 | the same |

### A conversation taken up by a fork

Main has $5 and $0.50 per conversation. The fork has no budget of its own, so it spends from main's.
Main makes 3 turns of one conversation, the fork takes it up and tries 6, then main tries 3 more.

| A fork's copy of a conversation | Turns that passed | That conversation, both computers |
|---|---|---|
| Starts at zero (#373's proposed text) | 3, 6, 3 | $1.10 |
| Starts at the parent's spend (`carry`) | 3, 3, 3 | $0.83 |
| Is the parent's (`shared`) | 3, 3, 0 | $0.55 |

In all three, main's computer ledger held every turn of both computers.

### No change to what #374 measured

#374's accuracy run, with Claude Code and the scripted client (Codex and Gemini CLI are not
installed here): the 81 calls with the provider's count were counted at $5.713961 against
$5.713962 billed. The overshoot rows above match #374's to the cent.

## The routes of #351, one by one

| #351's route | Where it stands |
|---|---|
| R1 A real key in a file, R2 a program that picks up a key by itself | Held by #374: a credential the computer was not given is refused on every host Silo reads |
| R3 A provider Silo does not read | Held by #374 for the hosts on its list. Inside a host Silo reads, a Chat stream with no usage is the same case: refused under a budget here |
| R4 The long tail of model hosts | Open. A list cannot be complete; only a limited network covers it (#351) |
| R5 Key formats | Not a route of spend; #351's scanning recommendation stands |
| R6 Keys the placeholder cannot replace (AWS signatures for Bedrock) | Open, not built here. Silo would hold the key and sign each request itself (#369) |
| R7 One credential, two uses (a GitHub token for GitHub Models) | Covered by R3's rule once `models.github.ai` is on the list of hosts Silo cannot count; not run |
| R8 A relay the agent deploys, R9 spend that runs somewhere else | Not closable at the boundary (#351) |
| R10 Tools the provider runs and bills | Held here for OpenAI's web search and Gemini's grounding, and by #374 for Anthropic's. Code execution billed by container time was not built |
| R11 Metered things that are not models | Not closable at the boundary (#351) |
| R12 A key pasted in a prompt | Held when the key reaches a header (R1's rule); not otherwise |
| Batches, background responses, cache storage (#351's "billed after the connection ends") | Held here, as work out of sight |

## What this changes in the plan

- **`budget-and-work-out-of-sight`** (new, open). How a budget counts background responses,
  batches and cache storage. Recommended: hold the bound until the result passes Silo.
- **`what-a-budget-promises`.** A fourth option: hold the typical output and cut only what passes
  the budget anyway. Recommended if "never over" is wanted. Also: the input check must know when a
  conversation's cache has expired.
- **`program-that-cannot-be-read`.** A Chat stream without usage is traffic Silo cannot count, on a
  host it reads. Recommended: refuse it under a budget.
- **`what-traffic-gives`.** Searches billed per call are outside the token counts; a request that
  names stored context is billed for it; a `queued` reply has no count yet.
- **`what-a-conversation-budget-promises`.** A fork's copy of a conversation: recommended to start
  at the parent's spend. This challenges #373's proposed text for `recognising-a-conversation`,
  that a fork's conversations start at zero, on the case above.

## Not verified

- Every provider here is a stand-in. Its formats follow the providers' documentation as read on
  25 November 2026 (OpenAI's background, batch, conversation-state and web search guides;
  Anthropic's batch and prompt-caching pages; Gemini's caching, batch and Search grounding pages),
  but no real provider was called: there is no provider key in this container.
- Gemini's storage price per million tokens per hour was not on the page read; $4.50 stands in.
  Whether a cache's creation is also billed as input was not stated either; the stand-in does not.
- Whether OpenAI bills a cancelled background response for what it generated is not stated; the
  stand-in bills it, and reports it in the cancel's reply.
- `groundingMetadata.webSearchQueries` is the field Gemini's `generateContent` replies use for
  queries as far as this agent knows; the page read shows the Interactions API instead.
- A background response streamed and resumed with `starting_after`, Gemini's batch mode, a batch
  cancelled midway, and code execution billed by time were not built.
- The time-based estimate of a call with no count is only as good as the speed it assumes; here
  it was the stand-in's own speed, so its accuracy is flattered.
- Which agent programs send Chat streams without `include_usage` was not captured.
