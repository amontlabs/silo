# Whose model reads the summary

## The choice

An agent's part, 25 November 2026, from `research/2026-11-25-reader-model`. You run Claude Code
on your Max plan in main, and install the reader of `agent-summary-checked`. Each morning it reads
every run's last words beside the run's receipt, three model calls a run. Your plan's sign-in
passes through the boundary with every request, but `silos-own-model-calls` keeps the reader off
it. So the reader asks some other model, and that model sees what you asked, what the agent said
and the facts of the receipt: commands, paths, hosts, exits.

The options, measured on #387's 75 claims (57 false, 16 true) with #400's prompts:

- An API key you give the reader, a provider's small model (recommended). Read twice, a small
  Anthropic model caught or flagged 55 of the 57 false claims, and said mismatch on 1 of the 16
  true ones; a mid-size one 51 and 0 (#400). The three calls hold about 2,000 tokens in and 300
  out. At list prices that is $0.0003 a run with Claude Haiku 5.5, $0.0002 with GPT-5 nano, $0.007
  with Claude Sonnet 5.5; 50 runs a night for 30 nights is $0.52, $0.32 and $10. A run in #371's
  night cost the agent $0.13 to $0.16, so the reader adds about a fifth of a percent with a small
  model. It costs an API key and its bill, and the reader sends the ask, the words and the receipt
  to that provider. The words already went to the agent's own provider; with a key from the same
  provider, what is new to it is the receipt.
- A model in the reader's own computer. Nothing leaves the Mac and no key is needed. Measured with
  a 4B model (Qwen3 4B, Q4, 2.5 GB) on 4 processor cores with no GPU: read twice, it caught or
  flagged 55 of 57, as the small hosted model, but said mismatch on 6 of the 16 true claims
  against 1. Its first call said "the words speak to the checker" on all 75 summaries, honest
  ones included, so the rule of `agent-summary-checked` that keeps a "matched" off such a summary
  would hold every run under What needs you; with this model the reader must not show that
  signal. A run took 40 seconds of the computer's processor. It costs a model file of a few
  gigabytes in the reader's recipe, the memory while it reads (a Mac runs few computers), and
  more false alarms. A A larger local model is not
  safer: Qwen3 8B caught or flagged 43 and said mismatch on 4 true claims, so whichever model
  ships has to be measured first. A GPU inside a computer goes through libkrun's virtio-gpu (Venus), which
  Podman's krunkit uses for llama.cpp on Macs; it was not tried here.
- A model on your Mac, such as Ollama, through the channel of #384. Free and private, on the Mac's
  GPU. It costs a change to `nothing-shared-from-your-device`, which forbids it today: the
  reader's computer would reach a server of your device. #384 already asks polarzero for that
  change in the notes of `program-that-cannot-be-read`; if it is taken, this option needs nothing
  more than the reader saying which model to ask.
- No model. The receipt's own lines and fixed phrases caught or flagged 17 of 57 (#387). It costs
  most of what the reader is for.

Recommended: an API key, with a model in the reader's own computer as the other choice the
install offers. The key gives the fewest false alarms for a fifth of a percent of what the agents
spend. The model in the computer is the only way that sends nothing out and changes no property,
and its false alarms cost a look, since an opinion changes nothing (`never-shown-as-a-fact`).
The plan's sign-in is not an option: `silos-own-model-calls` says why.

## Notes

An agent's part, 25 November 2026.

- `silos-own-model-calls` says a classifier with no key "does not ask" a model. That leaves out a
  model that needs no key because it runs in the reader's own computer. Its answer would need "a
  model that runs in a computer of Silo's, or on your Mac when that is granted, needs no key".
  This is noted on that entry.
- Both choices are the extension's own recipe: the key is a secret it names
  (`extension-secrets`), and the model file is part of the computer it ships
  (`extension-ships-a-computer`). Its calls to a provider leave through the boundary like any
  computer's, so they are counted and fall under its budget (`extension-spend`).
- Whether the receipt sent to a provider can hold a secret: a computer holds placeholders, not
  the secret values (`secret-placeholder`), so a command line that holds a token holds the
  placeholder. Not checked on a real receipt.
- Prices are list prices of 25 November 2026; a provider's batch price halves them, and the
  morning can wait for a batch.
