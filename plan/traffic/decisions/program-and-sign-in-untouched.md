# Does Silo leave the sign-in to the agent program

## For the owner

An agent's part, 22 November 2026. You make a fresh computer for a task and want Claude Code in it,
on your Max plan.

The options:

- Leave the sign-in to the program (recommended). You sign in inside the computer through
  Anthropic's own page, as on any new machine. Silo passes the sign-in on with each request and
  keeps it nowhere. It costs signing in again in each fresh computer.
- Silo holds the sign-in and puts it in. You sign in once on your Mac, and Silo adds your sign-in to
  the requests of every computer, as it does for an API key. It costs a conflict with Anthropic's
  terms, which forbid a developer to collect, store or intermediate a subscription's sign-in. One
  large product does it anyway, with no public agreement with Anthropic.

Recommended: leave the sign-in to the program. It is the path Anthropic's terms describe, and an
account at risk costs more than signing in again.

## Notes

Put forward so that the boundary reads inside the words of the vendors' terms. Logins themselves are open, in `agent-programs`.

## What vendors do and say

- Anthropic's own documentation recommends running Claude Code in a virtual machine, and lists "cloud instances, local hypervisors, and microVMs". It names another company's product for it. (Read on 19 November 2026.)
- Several products, hosted and local, come with Claude Code already installed and let the user sign in inside. That is this design's path.
- Other products go further: at least one sandbox keeps a subscription's sign-in outside the machine and injects it. That shows what a vendor ships, not what the terms allow, and this design does not lean on it.
- Anthropic's terms forbid a developer to "collect, store, or intermediate" a subscription's sign-in: `anthropic-reads-intermediate`.
