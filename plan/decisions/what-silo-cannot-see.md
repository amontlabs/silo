# What Silo cannot see

## The choice

An agent's part, 27 November 2026. `one-position` says what the position outside the computer
gives. A reader who believes it will assume Silo sees everything, and the first time it does not,
they stop believing the rest. Each limit below is already in an entry of the plan. None of them
is said in one place.

| Not seen | Why | Entry |
|---|---|---|
| What a model intended | Reasoning is sent sealed, as a summary, or in plain text, as the provider chooses | `hidden-reasoning-stays-closed` |
| What a provider's servers do | A web search the provider runs never crosses the boundary | `tools-the-provider-runs`, `vendor-runs-the-model-calls` |
| What an unread connection carries | Silo ends the encrypted connection only for the hosts it reads | `ending-the-encrypted-connection` |
| Which files were read | A build reads everything, so reads are not recorded | `file-reads` |
| A file made and deleted between two checkpoints | File changes compare the disk at one checkpoint with the one before | `file-changes-from-outside` |
| Which process did it | The probe runs inside, where an agent with full access can reach it | `processes-from-inside-linux`, `processes-from-inside-macos` |

The options:

- Say it in one entry, and show it in the product (recommended). The home page and the record say
  what is not seen. It costs a page that admits limits next to products that admit none.
- Leave each limit in its own entry. It costs nothing to write. A reader finds the limits one at a
  time, after trusting the record for something it does not hold.

Recommended: say it in one entry. `holes` already says Silo never fills a hole.

## What Silo can know of intent

An agent's part, 27 November 2026. polarzero asked that an extension compare what a model says
with what happens outside, and notice when "the model's intent isn't aligned with what's really
happening". What crosses the boundary in a model call, and is a fact once Silo reads it: what you
asked, the system prompt, every tool result that entered the model's context, every tool call the
model asked for, and the text the model wrote to you. So an extension can give an opinion on three
things: whether what the agent said it did matches the record (`said-compared-with-done`), whether a
tool call follows from what you asked (`whose-instruction-a-call-follows`), and whether a value
planted for the purpose was used (`planted-values`).

What stays out of reach is what the model intended and did not say. Published work shows that a
model's written reasoning does not always give its real reasons, so reasoning is a clue for a
classifier and never a fact about intent.

To find out: how much reasoning is readable at the boundary, for each agent program. An agent
searched the captures of the research and found no reasoning block. The run on real providers kept sizes and counts, and
every later run used stand-in providers that sent none.
