# How the core asks a driver to act

## The choice

An agent's part, 22 November 2026. The engine asks for a checkpoint of main. The part that takes it
hangs and answers a minute late, or the engine crashes and asks again after its restart.

The options:

- Ask, and hear back later (recommended). The engine never waits on the work. It sends the ask with
  an id, carries on with the other computers, and takes the answer or a timeout when it comes.
  Asking twice makes one checkpoint. An answer that comes after its timeout changes nothing by
  itself: the engine looks at what really exists. It costs an occasional wrong first report, when an
  action is shown as timed out that did finish, until the engine has looked.
- Wait for the answer. The engine asks and waits, as an ordinary program does. It is simpler, and it
  never reports a timeout wrongly. It costs every other computer while one action hangs, and a
  second checkpoint when an ask is repeated after a crash.

Recommended: ask, and hear back later. One stuck computer must not freeze the others.

## Notes

- The record's line reads "An effect is a request, not a call." "Request" is kept for what waits
  at the boundary, so the line is written "An effect asks a driver".
- The record checks `no-effect-twice` with the simulator, with crashes.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

The answer's last sentence was added on 22 November 2026, from these lessons.

- **Ids are never reused.** An id Silo allocates comes from a counter that only grows. When the ids
  run out, allocation fails with a named error; an id is never handed out twice, and one rolled back
  is never reissued. For the ids Silo allocates, such as effect ids and checkpoint ids. Call ids are
  chosen by the caller (`call-ids`).
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/storage/journal.zig#L63))
- **A reply matches its effect, once.** Every request that suspends has exactly one kind of reply,
  handled by a `switch` with no `else`. A reply is matched to the pending request before it is used,
  and is accepted once. Mini resumes with any reply, unmatched, and its resume test is disabled. In
  Silo a result or a timeout completes its pending effect once, matched by id, kind and the
  computer's restore count. A result that matches nothing, or comes after its timeout, completes
  nothing: what it reports is taken as an observation, like a virtual machine that dies by itself
  (`engine-reconciles`).
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/STATE-MANAGEMENT.md?plain=1#L46))
- **Nothing half-done at a wait.** Leave the state complete and named before waiting, and release
  what a waiting request holds when its outcome arrives, not by scope (`defer` does not span a
  wait). Mini popped its stack before yielding, so resuming broke. A step that issues an effect
  leaves its row in a named waiting state.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/src/async_executor.zig.md?plain=1#L210))
- **One gateway for every effect.** Send every visible effect through the one function that holds
  its guards. No second path writes directly. Every effect leaves the core through one function,
  where its id, kind and once-only key are checked.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/BUG_HUNT_REPORT.md?plain=1#L300))
