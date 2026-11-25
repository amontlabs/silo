# Does Silo read the traffic of an agent program signed in with a Google account

## The choice

An agent's part, 24 November 2026. You run Antigravity CLI in main, signed in with your Google
account. Silo could read its traffic as it reads Claude Code's, for spend, budgets and the record.

The options:

- Pass it unread (recommended). Spend, budgets and the conversation in the record do not cover
  that program while it uses a Google sign-in. With a Gemini API key or Vertex AI they do. It costs
  those features for people who use Gemini on a Google plan.
- Read it, as for the other providers. It costs a risk to your Google account: Google's terms
  forbid "third-party software, tools, or services" to reach these services with their sign-in, a
  Gemini CLI maintainer added "proxies" to that, and Google suspended accounts in waves as late as
  5 and 6 November 2026. A second suspension is for good.
- Read it, and ask Google first. It costs a wait with no end in sight: Google has answered such
  questions only in a GitHub discussion, and appeals went unanswered for months.

Recommended: pass it unread. Nothing else in Silo depends on reading it, and an account lost to a
ban costs more than the features.

## Notes

What the terms say, read on 24 November 2026 (an agent's reading, not a lawyer's):

- Gemini CLI's terms (updated 10 April 2026): "Directly accessing the services powering Gemini CLI
  (for example, the Gemini Code Assist service) using third-party software, tools, or services (for
  example, using OpenClaw with Gemini CLI OAuth) is a violation of applicable terms and policies."
- Antigravity's terms, clause 6: "Using third party software, tools, or services to access the
  Service (e.g. using OpenClaw with Antigravity OAuth) is a breach of this Agreement."
- A Gemini CLI maintainer, 27 February 2026, in google-gemini/gemini-cli discussion 20632: the terms
  bar "the use of 3rd party tools or proxies to access Antigravity resources and quotas".
- On geminicli.com: "Unpaid tier and Google One users: Gemini CLI was replaced by Antigravity CLI on
  June 18th, 2026."
- Against this reading: Gemini CLI's enterprise page documents routing "all outbound traffic through
  a corporate proxy", and Google's own way to limit sign-ins to a company domain is a proxy that
  intercepts requests to google.com. The cases enforced are gateways whose own code used the token.
  So the terms may well allow a proxy that carries Gemini CLI's own requests. The recommendation
  does not rest on the terms forbidding it, only on the risk.

Anthropic, OpenAI and GitHub document inspecting proxies for their programs, so this entry is about
Google alone. `subscription-and-key-alike` already lets a kind of sign-in pass unread when a
provider's terms rule it out.
