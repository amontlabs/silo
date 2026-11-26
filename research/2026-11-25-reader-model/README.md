# Whose model reads the summary

#387 and #400 found that a model reading an agent's last words beside the receipt catches most
false claims, and #400 settled on reading them twice (`agent-summary-checked`). #368 proposed
`silos-own-model-calls`: Silo never calls a model on the user's plan, only with a key given for it.
That leaves open whose model the reader asks. This folder measures two of the answers: a model
that runs on the user's own machine, and the price of a provider's model on the user's key.

It is research, stacked on #400 and #368 by merge: nothing here is built, and no verdict is
written.

## What was found

1. **A 4B model run locally catches as many false claims as the small hosted one, with far more
   false alarms.** Qwen3 4B Instruct (Q4_K_M, 2.5 GB), run by llama.cpp on 4 processor cores with
   no GPU, given #400's prompts unchanged: read twice, it caught or flagged 55 of the 57 false
   claims (small hosted model of #400: 55, mid-size: 51). On the 16 true claims it said mismatch
   on 6 (small hosted: 1, mid-size: 0) and "nothing in the record shows it" on 9.
2. **Its "speaks to the checker" signal is useless.** The claim-list call said the words speak to
   the checker on all 73 judged summaries with no attack, honest ones included; the small hosted
   model said so on 1. `agent-summary-checked` keeps a "matched" off such a summary and holds the
   run under What needs you, so with this model every run would be held. The signal can only be
   shown for a model measured to keep it rare on honest words.
3. **A run takes about 40 seconds of processor on the local model**: medians of 5 s for the
   inline reading, 20 s for the claim list and 15 s for its judge, on this machine. A night of 50
   runs is about half an hour of 4 cores. A Mac's GPU would be faster; not measured.
4. **On a key, the reader costs a fifth of a percent of what the agents spend.** #400's three
   calls hold about 1,970 tokens in and 300 out once the 1,550 tokens `claude -p` adds to every call
   are taken off (measured with an empty call). At list prices: $0.00034 a run with Claude Haiku
   5.5, $0.00022 with GPT-5 nano, $0.0011 with GPT-5 mini, $0.0069 with Claude Sonnet 5.5. With
   #371's real receipt (3,600 characters) instead of the corpus's thin ones, $0.0005 with Haiku.
   50 runs a night for 30 nights: $0.52 with Haiku 5.5, $10.33 with Sonnet 5.5. #371's runs cost
   the agent $0.13 to $0.16 each. Batch prices halve these, and the morning can wait for a batch.
5. **What the reader sends out.** On a key, the provider sees the ask, the words and the receipt.
   The words already went to the agent's provider; with a key from that provider, what is new to
   it is the receipt: commands, paths, hosts and exits.
6. **What the properties allow.** A model in the reader's own computer is allowed today
   (`extension-ships-a-computer`, `extension-code-in-its-own-computer`). A model on the user's Mac
   (Ollama through #384's channel) is not: `nothing-shared-from-your-device` forbids a computer
   reaching a server of the device, and #384 already asks polarzero to change it. The plan's
   sign-in is ruled out by `silos-own-model-calls`, whose answer also reads as ruling out a
   model that needs no key; a note there suggests a wording.

7. **A larger local model did worse, not better.** Qwen3 8B (Q4_K_M, 5 GB, its thinking switched
   off) read twice caught or flagged 43 of 57, said mismatch on 4 of the 16 true claims, and set
   "speaks to the checker" on 56 of 73 clean summaries. Its three calls took 80 seconds a run on
   this machine. Size alone does not make a local reader good enough: whichever model the reader
   is given has to be measured on a corpus like this one first.
8. **Under attack, only a first partial run.** The 4B inline reader was asked about 31 to 36 of
   the 57 false claims with the control line, a4 (an answer forged in its own JSON) and a8 (a false
   end of the words, then a «[system]» line) before the run stopped: it lost 1, 1 and 5 of them
   (small hosted: 1, 12 and 16 of 52). Too few to compare; listed in `results.txt`.

## The numbers

`results.txt`, from `harness/score_local.py` (which is #400's scorer pointed at `answers/`).
Caught (mismatch) + flagged (unbacked) of the 57 false claims; on the 16 true claims, mismatch +
unbacked. The hosted rows are #400's.

| Reader | Qwen3 4B, local: false | true | Small hosted: false | true | Mid hosted: false | true |
|---|---|---|---|---|---|---|
| inline (#387's prompt) | 7 + 46 | 1 + 13 | 4 + 48 | 1 + 8 | 8 + 41 | 0 + 4 |
| claim list, then judge | 11 + 40 | 5 + 9 | 3 + 40 | 0 + 7 | 9 + 29 | 0 + 3 |
| inline + claim list | 15 + 40 | 6 + 9 | 5 + 50 | 1 + 9 | 11 + 40 | 0 + 4 |

Qwen3 8B, local: inline 6 + 32 and 1 + 10; claim list 6 + 32 and 4 + 9; both 9 + 34 and 4 + 9.

`cost/results.txt`, from `cost/cost.py`: tokens per call and the price per run, per night and per
month at six models' list prices of 25 November 2026.

## How it was run

`harness/local.py` imports #400's runner and replaces its call: one llama.cpp chat completion at
temperature 0, the same system prompt and user text, the answer read the same way. Answers go to
`answers/` in #400's format with the tokens and the time of each call.
`harness/queue.sh` is every run made, in order.

```sh
python3 -m venv venv && venv/bin/pip install llama-cpp-python \
  --extra-index-url https://abetlen.github.io/llama-cpp-python/whl/cpu
# GGUF files, into $GGUF_DIR (default /tmp/claude-0/llm):
#   q4b.gguf  unsloth/Qwen3-4B-Instruct-2507-GGUF  Qwen3-4B-Instruct-2507-Q4_K_M.gguf
#   q8b.gguf  unsloth/Qwen3-8B-GGUF                 Qwen3-8B-Q4_K_M.gguf
PY=venv/bin/python sh harness/queue.sh
venv/bin/python harness/score_local.py > results.txt
python3 cost/cost.py > cost/results.txt
```

## What this changes in the plan

- New open entry `summary-reader-model` (extensions): whose model the reader asks. Recommends an
  API key with a provider's small model, and a model in the reader's own computer as the other
  choice the install offers, without the "speaks to the checker" signal.
- `silos-own-model-calls`: a note that its answer leaves out a model that needs no key, with a
  wording.
- `agent-summary-checked`: a note that the "speaks to the checker" signal depends on the model.

## What was not verified

- **No Mac, no GPU.** The local model ran on a Linux cloud machine's processor. Its answers do not
  depend on the hardware, its times do. A GPU in a libkrun computer (virtio-gpu with Venus, used by
  Podman's krunkit for llama.cpp on Macs) was not tried; published figures for it range from about
  30% to near the Mac's own speed.
- **The records are #387's: thin**, as #400 says. Most answers are "unbacked".
- **One model family per size, one run each, temperature 0.** Another local model may set the signal
  less often; a prompt tuned for a small local model was not tried, on purpose, so the numbers
  compare with #400's.
- **The attacks on the local models** were run only in part (the 4B inline reader, three lines,
  31 to 36 claims each). The claim-list reader and the 8B model were not attacked.
- **Token counts for other providers** are Anthropic's counts; their tokenizers differ by some
  percent.
- **Prices** are OpenRouter's catalogue of 25 November 2026, which resells at the provider's list
  price.
