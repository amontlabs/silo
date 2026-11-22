# How does anything change the engine's state

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

The answer's last sentence was added on 22 November 2026, from these lessons.

- **Validation produces a type.** Decode and validate untrusted input once, before the hot path,
  into a form the rest of the code trusts. Put that validation in the one front that every path goes
  through. `validate(raw)` returns an `Input`, and the core accepts only `Input`. Zig has no private
  fields, so the check that reads the source refuses building an `Input` anywhere but in the
  validation's file. A test checks that a rejected input never reaches the core or a driver.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/bytecode/README.md?plain=1#L42))
- **Unchecked steps are marked and proven.** Offer checked operations by default. An unchecked
  variant shows its precondition in its name or type, and every unchecked cast carries a SAFETY
  comment naming the precondition. Guillotine marks these with an `_unsafe` suffix. Silo can use a
  type that only the check produces, and keep the SAFETY comment for the few casts left.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/stack/stack.zig#L96))
- **Unknown input ends safely.** Parse names by exact match into an enum and return nothing for an
  unknown one. Unrecognized input maps to a terminating case, never to meaningful work. A
  well-formed name you do not know is answered as unknown; a malformed one is refused. An unknown
  kind of report from the probe is refused with a named error and recorded, never guessed.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/HARDFORKS.md?plain=1#L402))
- **Internal and public names never collide.** Keep internal identifiers in a namespace that public
  ones cannot reach, checked when compiling, and keep retired ones reserved. Inputs from the clock
  and from drivers are types that a call from a client cannot produce; the source check refuses
  building them outside their file.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/performance/SYNTHETIC-OPCODES.md?plain=1#L209))
