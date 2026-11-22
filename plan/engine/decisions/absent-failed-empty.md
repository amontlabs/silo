# What does a read that can fail return

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

- Guillotine's `get_balance` turns a failed database read into a balance of 0
  ([evm.zig](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/evm.zig#L1603)).
  Its callers cannot tell a real zero from a failure.
- In Silo, a probe that cannot read a file returns a failure, which becomes a gap line, never
  "no change" (`losses-show`).
- In Zig, such a read returns `Error!?T`: the error union carries the failure, the optional carries
  "nothing".

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Translate errors at each boundary.** Turn a dependency's errors into the local named set where
  they cross a boundary, explicitly, case by case. `engine-states-and-failures` asks for named
  failure sets and exhaustive handling. This adds translating a dependency's errors into the local
  set, case by case, where they cross. Guillotine lost errors at its C boundary.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/frame/frame.zig#L186))
- **Small, honest error sets.** An error set lists only failures the code can produce, one name per
  condition. An open set (`anyerror`) only where an implementer brings their own errors. Keeps every
  `switch` over failures meaningful: each arm is a case that can happen.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/storage/README.md?plain=1#L501))
- **One bad input, one error.** Where a result reports a single failure, define which failure wins,
  so the same bad input always gets the same error. The same input then always gives the same
  refusal, which the simulator can compare.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/CLAUDE.md?plain=1#L666))
