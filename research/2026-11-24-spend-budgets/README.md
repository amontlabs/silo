# Spend budgets at the boundary, built and measured

Research of 24 November 2026, by an agent, for polarzero. #351 designed budgets on paper and
checked them with a simulator. This folder builds them into the Zig boundary of #347 and runs
real agent programs through it: Claude Code 2.1.331, Codex 0.172.0 and Gemini CLI, against
stand-in providers that bill every call. The question is whether a budget holds when real
programs spend, and how far past it they get.

Nothing here is a decision, and nothing under `plan/` changed except agents' notes. The findings
are copied into the notes of the plan entries they bear on, listed at the end.

## In short

- **The count is exact wherever the provider gives one.** Over 140 calls from four programs, the
  134 calls that ended with the provider's own count were counted at $8.465880, and the stand-in
  billed $8.465877. Anthropic's fallback attempts are counted only by reading
  `usage.iterations`: the top-level counts alone would have missed 10% of those calls' cost.
- **Without the provider's count, the boundary undercounts by about 80%.** Three Chat
  Completions streams without `include_usage` and three calls the program dropped were billed
  $0.20. The boundary, counting the visible output at four characters a token, put down $0.04.
  The difference is reasoning, which streams as nothing the boundary can see.
- **"Calls under way finish" goes far past the budget with subagents.** Claude Code with 8
  subagents under a $3 budget on Opus 5.5 ended at $4.44 to $4.61, 54% over at worst. A third to a
  half of that came from calls that left after the budget was truly reached, because their
  reasoning had not been counted yet. Counting a running call's output by time removes that part,
  and leaves the calls already under way: 35% over at worst. Fable 5.2 under $6 gave the same
  shares, 56% and 34%.
- **Holding the typical output stops just under the budget, and holding `max_tokens` stops the
  agent at once.** Holding each call's input and the 90th percentile of its model's output ended
  every run 7% to 9% under the budget, never over. Holding `max_tokens` stopped Claude Code after 3
  calls, at 15% to 18% of the budget: Claude Code asks for 128,000 output tokens on every call.
- **The input check matters for one call, not for a night.** A conversation sent a 2.4 MB tool
  result with $0.34 left of $1. Under "calls under way finish" it left, and the budget ended at
  $7.38. With the input check it was refused.
- **The 400 in the provider's own shape stops every program and shows Silo's sentence**, as #369
  found with a stand-in. Here it was Silo's boundary under a real budget: Claude Code, Codex and
  Gemini CLI stopped on the first refusal and printed it. A key the computer was not given, and a
  model with no price under a budget, stopped Claude Code in 0.5 s with the reason, with no retry.
- **A rate that answers 400 is a stop, not a rate.** Both programs ended their task on the first
  refusal. Answered with 429 and `retry-after`, the same rate slowed them instead: Codex finished
  its whole task in 148 s, waiting out each window, and Claude Code kept working until the run's
  limit.
- **A budget on a person holds across computers.** Two Claude Codes and one Codex of one person,
  under $2.50 together, all stopped, at $2.92.
- **A plan's window can be capped, but not split between computers.** A cap of 50% of the 5-hour
  window stopped Claude Code under a subscription at 51%. The share the boundary put on that
  computer, 0.19 of the window, held 0.06 that other devices of the account had used.
- **Claude Code says which kind of credential a call uses, in its headers.** With an API key it
  sends `x-api-key`. Signed in with a subscription token it sends `Authorization: Bearer
  sk-ant-oat01-…` with the `oauth-2025-04-20` beta. So a budget can tell metered calls from plan
  calls before a call leaves. #351 left this open.

## What was built

The boundary of #347 ends the TLS connection a program opens to a provider, opens its own, and
forwards every byte as it came. This copy adds, in `boundary/src/`:

| File | What it does |
|---|---|
| `usage.zig` | Reads the token counts of one call from a copy of its reply, for Anthropic Messages, OpenAI Responses (HTTP and WebSocket events), OpenAI Chat Completions and Gemini (SSE, a JSON array, or one JSON body). It takes cache reads, cache writes by lifetime, reasoning, web searches, `usage.iterations`, `usage.speed` and the tier, and counts the visible output while the reply streams. |
| `prices.zig` | Silo's own price table, in [`prices.json`](prices.json): one row per provider, model, tier and date of effect, with long-context rates. The file is read again when it changes, so a new model or a price that changes on a date needs no restart. A model with no row has no price. |
| `budget.zig` | Ledgers for each computer and each person: total, rate over a window, alert at 80%, plan windows. A call is admitted against every ledger that covers it, counted while it runs, and settled on the provider's count. Four admission policies, below. |
| `spend.zig` | What the boundary reads from a request before it leaves: model, `max_tokens`, tier, the credential, and a hash of the conversation at each of its items. The refusal in each provider's shape, 400, or 429 with `retry-after`. |
| `main.zig` | The boundary of #347, with the above wired in. It tells computers apart by the address their connection arrives on. |

Before a model request leaves, the boundary:

1. refuses it if its credential is not one this computer was given;
2. refuses it if a budget covers the computer and the model has no price;
3. estimates its input: from the provider's count for the longest earlier request it continues,
   plus its new bytes, or from its size alone for the first call of a conversation;
4. admits it against every budget of its computer and person, or refuses it.

A known model host it cannot count (`api.mistral.ai` in the tests) is refused whole while a
budget covers the computer.

The admission policies:

| Policy | A call leaves when |
|---|---|
| `finish` | what is counted (settled calls, and the input and visible output of calls under way) is under the limit. The plan's answer today. |
| `finish` with `output_tps` | the same, and a running call's output also counts by time, at the model's speed, up to its `max_tokens`. Hidden reasoning counts while it happens. |
| `admit_input` | the same, and the call's own input, at the dearest rate it can incur, fits in what is left. #351's proposal. |
| `reserve_typical` | each call under way holds its input and the 90th percentile of its model's output, and the new call's hold fits. |
| `reserve_max` | the same with `max_tokens` held, LiteLLM's default. |

## How it was run

Everything ran inside a private network and mount namespace (`run/run.sh`): the providers' hosts
point at the boundary through each program's own `/etc/hosts`, and Silo's test authority reaches
each program through its own environment. Nothing outside the namespace changed: no global trust
store, environment or firewall.

The stand-in providers (`fake/provider.py`) play a model that works through a long task: tool
calls in a row, a burst of parallel subagents for Claude Code, a large file read. They count
tokens with a stand-in tokenizer, cache prompts with Anthropic's markers and lifetimes and
OpenAI's and Google's automatic prefixes, think before answering (reasoning hidden from the
stream, as current models send it), sometimes fall back to another model inside a call, send
plan-window headers to a subscription sign-in, and bill each call in `ledger.jsonl`. A call the
program drops is billed for what was generated until then. `run/compare.py` prices the ledger
with the same table and compares it with the boundary's record.

Codex 0.172.0 tries a WebSocket to `api.openai.com` first. The stand-ins refuse it, and Codex falls
back to HTTPS. Codex now sends its tools as one JavaScript `exec` tool in an `additional_tools`
input item; the stand-in answers with code that calls `tools.exec_command`.

The stand-ins stream at 500 to 4,000 tokens a second, so a run takes about a minute. Real models
are 10 to 50 times slower, which makes every call under way at the limit longer and the
overshoot of `finish` larger, not smaller.

## Results

### The count against the bill

`run/configs/accuracy.json`, `run/scenarios/long.json`: Claude Code with 6 subagents, Codex,
Gemini CLI and the scripted client `run/client.py`, at once, on four computers.

| | Calls | Billed | Counted |
|---|---:|---:|---:|
| With the provider's final count | 134 | $8.465877 | $8.465880 |
| Chat Completions without `include_usage` | 3 | $0.101 | $0.019 |
| Dropped by the program while the model reasoned | 3 | $0.101 | $0.021 |

Ten calls of Claude Code fell back to another model inside the call. Their attempts cost $0.666,
and their top-level counts alone $0.598.

The estimate of a call's input before it leaves, against the provider's count afterwards:

| Estimate from | Calls | Error |
|---|---:|---|
| The conversation's earlier request | 108 | -8% to +3%, median 0.1% |
| The request's size (3 bytes a token) | 26 | +10% to +30% |

The stand-in's tokenizer is regular, so the second line flatters a real one. The first does not
depend on the tokenizer: it rests on the provider's own count of the earlier request.

### Overshoot

`run/overshoot.sh`: Claude Code with 8 subagents started at once, each making up to 14 calls with
2,000 to 12,000 tokens of reasoning. The same seed for every policy of one row of runs.

Opus 5.5, a budget of $3:

| Policy | Spent, each run | Over the budget, worst | From calls that left after the limit was truly reached, worst | Calls done, mean |
|---|---|---|---|---|
| `finish` | 4.44, 4.61, 4.55 | +1.61 (+54%) | 0.62 | 28 |
| `finish` with `output_tps` | 3.71, 4.05, 3.93 | +1.05 (+35%) | 0.00 | 25 |
| `admit_input` | 3.71, 4.05, 3.78 | +1.05 (+35%) | 0.00 | 24 |
| `reserve_typical` | 2.74, 2.78, 2.80 | -0.20 (-7%) | 0.00 | 18 |
| `reserve_max` | 0.44, 0.45, 0.53 | -2.47 (-82%) | 0.00 | 3 |

Fable 5.2, a budget of $6:

| Policy | Spent, each run | Over the budget, worst | From calls that left after the limit was truly reached, worst | Calls done, mean |
|---|---|---|---|---|
| `finish` | 9.24, 9.17, 9.36 | +3.36 (+56%) | 1.55 | 29 |
| `finish` with `output_tps` | 7.79, 8.06, 7.81 | +2.06 (+34%) | 0.00 | 25 |
| `admit_input` | 7.38, 8.05, 7.53 | +2.05 (+34%) | 0.00 | 24 |
| `reserve_typical` | 5.47, 5.55, 5.58 | -0.42 (-7%) | 0.00 | 18 |
| `reserve_max` | 0.89, 0.91, 1.07 | -4.93 (-82%) | 0.00 | 3 |

"Truly reached" is when the stand-in's bill, spread evenly over each call, passed the budget.
Whatever is over the budget and not in that column came from calls already under way at that
moment. `admit_input` changed nothing here: Claude Code's inputs are mostly cache reads. It
matters for one large input:

| `run/bigcall.py`, a budget of $1 | The 2.4 MB tool result | Spent |
|---|---|---:|
| `finish` | left, at $0.66 spent | $7.38 |
| `admit_input` | refused: "this request's input alone costs up to $5.14, and this computer's budget of $1.00 has $0.34 left" | $0.66 |

### Refusals the programs show

| Case | What happened |
|---|---|
| A budget reached (Claude Code, Codex, Gemini CLI) | Each stopped on the first refusal it got and printed Silo's sentence, such as Claude Code's "API Error: 400 Silo: this computer's budget of $3.00 is reached ($4.44 spent). Raise it in the Silo app to go on." Gemini CLI, under $0.50, stopped at $0.53. |
| A key the computer was not given | Claude Code stopped in 0.5 s: "API Error: 400 Silo: this computer was not given this key, so it cannot be used here." No retry. Its calls for settings were refused too. |
| A model with no price, under a budget | Claude Code with `--model claude-opus-6-0` stopped in 0.5 s with the reason. No retry. |
| A host Silo cannot count, under a budget | The client got 400 in OpenAI's shape. |
| A rate, as 400 | Claude Code and Codex stopped their task at the first refusal, after 9 calls each. |
| A rate, as 429 with `retry-after` | Claude Code waited and went on (13 waits, 61 calls until the run's 240 s limit). Codex finished all 30 steps of its task in 148 s. The worst 30 s window spent $0.85 against $0.60, from calls under way. |
| A person's budget of $2.50 over three computers | Both Claude Codes and Codex stopped; $2.92 spent in all. |
| 50% of the plan's 5-hour window | Claude Code under a subscription stopped at 51%. |

## What this changes in the plan

- **`what-a-budget-promises`.** "Calls under way finish" is right, but only with a count that
  includes reasoning while it happens. Without it a third to a half of the overshoot is calls
  that should not have left. Recommended: count a running call's output by time, and keep the input check for
  the one large call. Either way the promise has to name its overshoot: here a third of the
  budget, with 8 subagents. If polarzero wants "never over", hold the typical output: it stops
  7% to 9% short.
- **`budget-as-a-rate`.** A rate refused with 400 ends the agent's task, so it is a second total,
  not a rate. Recommended: refuse by a rate with 429 and `retry-after`, in the provider's own
  rate-limit shape. The two programs waited and went on. A total still answers 400.
- **`what-traffic-gives`, `how-spend-is-shown`.** A call with no final count is not a rare edge:
  every Chat Completions stream that does not ask for usage is one, and so is every call a
  program cancels while the model reasons. Visible characters miss about 80% of them. A floor by
  time (seconds elapsed at the model's speed) is the better estimate, and the app should mark such
  spend as estimated.
- **`subscription-and-key-alike`.** The kind of credential is in the headers before the call
  leaves, for Claude Code too, so a budget on metered calls only is possible without reading
  anything the provider sends back.
- **`budgets-together`.** A person's budget over several computers holds like a computer's, with
  the same overshoot from calls under way.
- **Plan windows.** A cap on the account's window is exact. A share of it per computer is not:
  the provider reports the whole account, and the boundary cannot tell its own computer's use from
  the account's other devices. Recommended: offer the cap, not a per-computer share.

## Not built, or not measured

- Admission inside a WebSocket. Codex under a ChatGPT sign-in sends its calls as messages of one
  WebSocket, and a refusal then has to be a message in the socket. The boundary reads the
  events there, but refusing one call without closing the socket was not built.
- Batches, `background: true` responses, and tools billed by time (code execution, storage).
- The price table read again on change was tested by its unit test, not during a run.
- Real models' speed. The stand-ins are faster, which makes these overshoots smaller than real
  ones.
- A bug of the #347 spike, fixed here: a body that ends with the connection handed its last piece
  to the readers twice. Gemini's JSON-array streams then failed to parse, and the last events of a
  stream were read twice. It is in `pumpBody`.

## Running it

Linux, as root, with Zig 0.15.2, OpenSSL, zlib and zstd, Python 3, and the programs under test.

```
run/tls.sh                                     # test certificates, in run/tls
(cd boundary && zig build && zig build test)   # the boundary and its unit tests
run/run.sh accuracy run/configs/accuracy.json run/scenarios/long.json 300 cc cx gem script
python3 run/compare.py out/accuracy            # the count against the bill
run/overshoot.sh 3 claude-opus-5-5 1 2 3       # the overshoot table
python3 run/table.py claude-opus-5-5 3
```

Every key in `run/progs.sh` is a fake. The runs write to `out/`, which is not kept.

## Plan entries that got notes

`what-a-budget-promises`, `budget-as-a-rate`, `budget-denies-or-holds`, `what-traffic-gives`,
`how-spend-is-shown`, `subscription-and-key-alike`, `budgets-together`,
`program-that-cannot-be-read`.
