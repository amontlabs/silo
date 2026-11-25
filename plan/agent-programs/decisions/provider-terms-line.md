# Does Silo refuse to build what helps get around a provider's terms

## The choice

An agent's part, 22 November 2026. Your Claude subscription reaches its usage limit at 15:00. You
have a second subscription, and a tool could switch your agents to it each time the first one runs
out.

The options:

- Nothing that gets around the terms (recommended). Silo builds no account rotation and no sharing
  of a subscription, and asks a provider first when its terms are unclear. You can still do what you
  want with your own accounts inside your computers. It costs features some people ask for, and a
  wait for a provider's answer, as for using one subscription from every computer through Silo.
- Build what people ask for, and leave the terms to them. It costs the risk that a provider treats
  traffic that comes through Silo as abuse and refuses it, for every user of Silo.

Recommended: nothing that gets around the terms. Silo sits between your agents and their providers,
so a provider's trust in it is worth more than any one feature.

## Notes

One line of the earlier notes is in question, and is polarzero's to settle:

"Silo builds nothing that helps get around a provider's terms, such as rotating accounts".

## Every provider's terms, read against Silo (24 November 2026)

An agent's part, 24 November 2026. The terms of Anthropic, OpenAI, Google, GitHub Copilot, Cursor
and others were read that day, against each thing Silo does with a provider's traffic. This is a
reading by an agent, not a lawyer's.

- With an API key, nothing Silo does is against any provider's terms. Anthropic allows a key in "a
  development environment, secrets manager, or machine image". OpenAI's Codex proxy and Anthropic's
  sandbox-runtime both swap a placeholder for a real credential on the way out, as
  `secret-placeholder` does.
- Reading a plan's traffic is documented by Anthropic, OpenAI and GitHub, which describe inspecting
  proxies for their programs. Anthropic's Usage Policy of 12 November 2026 forbids to "Resell,
  proxy, or otherwise provide access to Claude through unauthorized means, including services that
  route requests through consumer subscriptions or misrepresent the product or client being used".
  Silo gives no one else access and does not pose as Claude Code, but "authorized" is not defined.
  It is the question for Anthropic in `anthropic-reads-intermediate`.
- Google forbids third-party software to reach Gemini CLI or Antigravity with their Google sign-in,
  and enforces it with suspensions: `google-sign-in-unread`.
- Holding a plan's sign-in outside the computer and swapping it in is forbidden by Anthropic's
  words ("collect, store, or intermediate") and Google's. OpenAI differs: its Codex page tells users
  to copy `~/.codex/auth.json` to a headless machine, and its Sign in with ChatGPT Terms allow
  storage "local and under the user's control". So `logins` is stricter than OpenAI asks. It stays
  as it is until OpenAI says yes in writing.
- No provider allows a tool to make its own requests on a user's plan: `silos-own-model-calls`.
- Every case a provider enforced in 2026 was a program that is not the vendor's using a plan's
  sign-in from its own code, often posing as the vendor's program. None was a proxy that carries
  the vendor's own program, unmodified, with the user's own sign-in. `disguising` and
  `forwarded-byte-for-byte` keep Silo in the second case, which makes them the line this entry
  draws.
- Anthropic's rule for products that run Claude Code adds one condition `recipe-under-preinstall-rule`
  does not name: "you can't use the Claude Code or Anthropic names or logos as part of your own
  product, feature, or company name". A recipe can say it installs Claude Code. A feature of Silo
  cannot be named after it.
