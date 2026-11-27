# How Anthropic reads "intermediate"

## The choice

An agent's part, 22 November 2026. You use Claude Code on your Max plan in main. Your sign-in passes
through Silo with each request, and Silo keeps it nowhere. Anthropic's terms forbid a developer to
"intermediate" a subscription's sign-in, and no text defines the word.

The options:

- Go on, and ask (recommended). Silo reads the traffic as a company's proxy does, which Anthropic's
  own pages describe for a subscription's sign-in. Silo asks Anthropic in writing before anyone else
  uses it. If Anthropic says no, Silo then reads Anthropic traffic only where it holds your API key
  itself. It costs a risk on your own account in the meantime, since Anthropic says it may act
  without notice.
- Wait for Anthropic's answer. Silo reads no subscription traffic to Anthropic until Anthropic says
  yes. It costs spend, budgets and tool calls for Claude Code on a plan until then, and the answer
  may never come.

Recommended: go on, and ask. Anthropic's own pages describe this passage, and while you are the only
user the risk falls on your account alone.

## Notes

To find out, by asking.

Read on 19 November 2026: Anthropic's terms forbid a developer to "collect, store, or intermediate" a subscription's sign-in. No text defines "intermediate". The token passes through the boundary, as it passes through a company's proxy, and Anthropic's own pages describe that passage.

## A clause of the Usage Policy of 12 November 2026 (24 November 2026)

An agent's part, 24 November 2026. The reading of 19 November rests on the word "intermediate".
Anthropic's Usage Policy, effective 12 November 2026 and read on the 24th, has a clause closer to
the case. Under "Do Not Abuse Our Platform" it forbids to "Resell, proxy, or otherwise provide
access to Claude through unauthorized means", and the same bullet names services that route
requests through consumer subscriptions, or misrepresent the product or client being used. The
policy names Claude Code among the apps it covers. The quotes and the other terms read are in
`terms-on-changing-a-reply`.

Silo's boundary carries the user's own Claude Code requests, from a computer the user owns, with
the real client named and nothing resold. Whether Anthropic counts that as an authorized proxy, the
text does not say. What changes for the answer: the question to Anthropic should name this clause
and not only "intermediate", since it is the one that names proxies and consumer subscriptions.
The fallback the answer gives, reading Anthropic traffic only where Silo holds an API key, stays
clear of the clause: the Consumer Terms exempt API keys from their rule on automated access, and
the Commercial Terms that govern a key say nothing of proxies.


## The fallback has nothing to go by under option A (25 November 2026)

An agent's part, from `research/2026-11-25-key-or-plan/`. The answer's fallback reads Anthropic
traffic "only in computers whose API key it holds as a secret". Option A of #373 leaves
`secrets-held-outside` out of the first build, so in that build Silo holds no key and the fallback
names no computer. `claude-key-or-plan` is the replacement: each computer is set for Claude with an
API key or with a plan, and one set for a plan passes Anthropic traffic on unread. It was built
into #374's boundary and run with real Claude Code. If polarzero takes it, the fallback sentence of
this answer would read:

> If Anthropic answers that this intermediates, Silo reads Anthropic traffic only in computers set
> for Claude with an API key, as `claude-key-or-plan` says.

That edit is not made here, since #373's `decisions.md` proposes its own edit to the same answer;
both fit in one text.

## What Anthropic's own pages say about reading (27 November 2026)

An agent's part. Another agent read these pages on 27 November 2026. They were not read a second
time for this note.

- The network page of Claude Code's documentation says TLS-inspection proxies "work without
  additional configuration when their root certificate is installed in the OS trust store"
  ([network configuration](https://code.claude.com/docs/en/network-config)).
- The gateway page describes a subscription's sign-in passing through a gateway: "a saved
  claude.ai login remains the active credential, so its usage limits and billing apply". It lists
  budgets and "log every model request" among what a gateway is for
  ([LLM gateway](https://code.claude.com/docs/en/llm-gateway)).
- The gateway compatibility guide says how to inspect: "inspect without modifying"
  ([guide](https://code.claude.com/docs/en/llm-gateway-protocol)). The boundary forwards byte for
  byte (`forwarded-byte-for-byte`), which is that.
- Claude Code can give out the same content itself, as a report: its setting
  `OTEL_LOG_RAW_API_BODIES` emits each request and reply
  ([monitoring](https://code.claude.com/docs/en/monitoring-usage)).

What this changes. The answer above already goes by the gateway page, and these pages support it
further. Writing to Anthropic is now to have that reading in writing, for a product and not for
one organisation's own network, which none of these pages covers.

What it does not change. The legal page still forbids developers to "collect, store, or
intermediate Claude.ai credentials or session tokens", and nobody at Anthropic has said what
"intermediate" covers. The fallback of the answer stays.

If Anthropic said no, less would be lost than `claude-key-or-plan` alone suggests. The record of
the disk, the network and the processes does not depend on any provider. The model's words would
come from Claude Code's own transcript, as a report where they are a fact today. What would be
lost is refusing a model call at the boundary, and knowing that the account of the conversation
is complete.
