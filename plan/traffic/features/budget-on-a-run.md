# A budget on a run

## For the owner

An agent's part, 22 November 2026. In main, Claude Code runs a refactor while Codex fixes tests. You
want the refactor to spend no more than 10 dollars.

The options:

- Leave it out (recommended). You run the refactor in a fork with its own budget of 10 dollars, or
  put the budget on its conversation. Both are exact. It costs moving a task you want limited on its
  own out of main, or a limit that does not cover the subagents it starts.
- Build it. You set 10 dollars on the refactor's run in main. It costs a limit that rests on Silo's
  guess of which run each model call belongs to. When the guess is wrong, the wrong work is paused
  or refused, and pausing main pauses Codex too.

Recommended: leave it out. A fork with a budget of its own gives the same limit exactly, and a
budget that can pause the wrong work is a poor limit.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

"It pauses the computer and alerts you" comes from the examples in the notes of `what-a-precaution-may-do`, not from a decision on budgets.
