# Extensions in sequence

## The choice

An agent's part, 22 November 2026. Main's agents make 2,000 tool calls a day. A pattern in Silo
sorts 1,900 of them as ordinary. A small model, added as an extension, looks at the other 100 and is
unsure about 6. Your own agent program, in an extension's computer, looks at those 6 and alerts you
about one.

The options:

- Build it (recommended). Each step wakes only when the one before is unsure, so the costly one sees
  a few calls a day. It costs time when a request waits for all three, since they share one limit
  and the last may not get its turn, and you set up each step. It also costs a change to
  what polarzero decided wakes an extension (`what-wakes-an-extension`), which is a time, a new line of
  the record, your click or a request that waits. The opinion of a pattern inside Silo is none of
  these.
- Leave it out. Each extension looks at everything it is woken for. It costs tokens: an agent asked
  about 2,000 calls a day, or only the small model's opinion.

Recommended: build it. Spam filters and fraud checks work the same way, cheap checks first and a
costly look last, because most of what they see is ordinary.

## Notes

Set aside for later by polarzero. The three tiers, as agreed before they were set aside:

> **Three tiers** (Agreed). Each wakes only when the one before cannot judge.
>
> 1. **The built-in classifiers.** In Silo itself, beside the rules. They always run and cost no
>    tokens.
> 2. **A small model.** An added classifier that sorts what the first tier cannot judge: normal,
>    risky, needs you.
> 3. **An agent.** An added classifier that wakes when the small model is unsure, or when you ask
>    it a question.

Put forward with `extension-manifest`: the tiers become one extension woken by another's opinion.

**The monitor agent** (polarzero's idea, in an agreed form). polarzero proposed an agent shipped
inside the app, with tools to use the app and look inside computers, not allowed to change files
or use the network, sitting at the app's level with the whole picture. The agreed form is the
third tier above: your own agent program, in its own computer, with read access to the record.
Silo ships the recipe, the instructions and the commands. It is not Silo's own agent and it does
not run beside the app. It gives opinions and answers your questions. It pauses nothing itself: a
precaution you set does.
