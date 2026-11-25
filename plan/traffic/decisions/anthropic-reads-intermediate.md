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

