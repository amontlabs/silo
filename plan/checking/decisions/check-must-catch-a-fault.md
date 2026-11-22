# When a check counts

A check that has never failed shows nothing: it may be looking at the wrong thing, or at nothing.

One example, from the trial described in the notes of `core-simulator`, 21 November 2026. The
code that gives a secret was changed on purpose so that a precaution could give one. The simulator
found it within 3 histories on average and printed the one call that shows it. Had it found
nothing, the check would have been the thing to fix.

The standard name is mutation testing. What differs here: one fault is put in by hand for each
check, where mutation testing changes the code automatically in many places.

The owner said yes on 21 November 2026.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **A check that cannot fail checks nothing.** Delete tests with no assertion, tests named for a
  case they never trigger, comparisons that only log, and assertions that cannot be false. Write an
  assertion as the property, stated positively. `check-must-catch-a-fault` catches these. The test
  that reads sources can also refuse a comparison whose only effect is a log line.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/stack/stack.zig#L528))
