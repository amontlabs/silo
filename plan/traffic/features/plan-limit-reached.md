# When a plan's limit is reached

## For the owner

An agent's part, 22 November 2026. At 14:20 Claude Code in main reaches your Max plan's 5-hour
limit, which Anthropic says resets at 17:00. Silo takes a checkpoint, alerts you, and alerts you
again at 17:00 that main's agents can go on.

The options:

- Build it (recommended). It costs one more alert, and nothing more than the first alert when the
  provider gives no reset time. Silo starts nothing again by itself.
- Leave it out. Claude Code shows the limit and the reset time in its own window. It costs you
  checking each program yourself to know when to come back.

Recommended: build it. It is small, it rests on what the provider said, and it saves you keeping an
eye on the clock.

## Notes

From the list of smaller features the owner put on the roadmap on 21 November 2026. The owner's
words on the list are in `what-ships-with-silo`.

The need: a run that reaches the limit of a plan waits until the limit resets, and people want to
know when to come back to it.

## An agent's notes

- The provider's answer is read at the boundary (`model-traffic-read`), so it is a fact and this is
  a rule. A checkpoint and an alert are both protective.
- Silo does not start the run again at the reset time. `what-silo-does-not-build` proposes no
  orchestration.
- `checkpoints-taken-on-their-own` takes checkpoints at moments the engine sees. This may be one
  more such moment and not an action of its own.
- Bad paths to design: the provider gives no reset time. The limit resets at another time than it
  said. The traffic cannot be read (`program-that-cannot-be-read`). Several runs reach the limit
  together, which should make one alert.
- An extension that adds rules could do this, if a rule can be triggered by this fact and by a time.
