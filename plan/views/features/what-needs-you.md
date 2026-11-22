# What needs you

## The choice

An agent's part, 22 November 2026. At 16:05 main asks to reach api.stripe.com and the request is
held for 90 seconds, the fork try-bun has written 41 GB, and claude #1 has finished in client-a.

The options:

- Build it (recommended). The app opens on that list, the ask on top with its time left, and you
  allow or deny it on its line. It costs strict ranking work: the list has to stay short and never
  reorder itself while you read it.
- Leave it out. The app opens on Now, and asks come as notifications. It costs the first screen you
  decided on, and asks that run out while you look elsewhere.

Recommended: build it. Asks with a time limit need one place you always see first.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

It reads `first-screen` and `one-queue` as the same list. If the queue is a place of its own, they are two features.

## polarzero's words (21 November 2026)

Asked whether this list should become the place where you follow and answer every agent's thread:

> phone is already planned, what needs you we do some part of this but on working with agents
> specifically that's the responsibility of the app typically T3 Code.

An agent's reading: this list holds what Silo itself waits on you for. The view where you work with
the agents' threads belongs to the client app, typically T3 Code (`t3-code-integration`,
`following-the-work`).

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Nothing in a view flickers.** Break ties in a ranking with a stable second key, and give a
  readiness signal a small tolerance band, so equal items keep their order and a state at its edge
  does not flip back and forth. Silo's short ranked list of what needs you must not reorder itself,
  and a computer near a limit must not blink between two states, between two refreshes.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/docs/context/%5Bpass%202/5%5D%20phase-5-txpool%20%28Transaction%20Pool%29.md?plain=1#L13))
