# What limits the engine's tables and memory

The record checks `nothing-grows-without-limit` with assertions.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

The answer's last sentence was added on 22 November 2026, from these lessons.

- **Limits as a struct checked at compile time.** Build each component's type from a settings struct
  with `Core(comptime limits)`. A `validate()` refuses bad values and bad relations between fields
  with `@compileError`, names the value, the bound and a value that would pass, and derives integer
  widths from the limits. The simulator runs the core at the product's sizes, so it checks the core
  that ships. The bounded exhaustive test builds the same code at the trial's small sizes (2
  computers, 2 secrets, 2 hosts, 3 rules).
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/evm.zig#L20))
- **Row sizes asserted at compile time.** Assert the exact size of every type whose size matters.
  With no allocation after start, the engine's memory is the limits times the asserted row sizes. A
  test prints the total, and a change to a row that grows it fails.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/preprocessor/dispatch_item.zig#L21))
- **Every loop has a bound with a reason.** Every loop driven by input has a bound of its own.
  Derive it from the budget it protects and write the reasoning next to the constant. TigerStyle
  already asks for limits on everything. Guillotine adds the written reason beside each limit.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/TESTING.md?plain=1#L719))
- **Cap counts as well as bytes.** Cap the number of entries in every growing list, not only its
  size in bytes. Each table has a count limit beside its byte limit. For the record, what happens at
  the limit follows `record-size-limit`: bulky content goes first, and a fact says what was removed.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/storage/REVIEW_SUMMARY.md?plain=1#L193))
- **Check everything before changing anything.** Compute the full cost and check every limit,
  including bookkeeping, before the first change. Allocate scratch space before the first change. An
  all-or-nothing operation is tested to leave the state untouched when it fails. Proposed property:
  an input the core cannot apply changes only the rows that say it was refused. A repeated call
  still returns the same refusal (`call-ids`), and a failed model call still counts its spend.
  Guillotine's database restore clears the live state and then rebuilds it with allocations that can
  fail, which is the case this rule prevents.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/GAS-METERING.md?plain=1#L568))
- **Reset per-run counters when a run starts.** Reset per-run counters and limits when the run
  starts, not only when the object is built. A pooled object that is reused resets every field a
  previous user could have set. Counters that belong to one run or one fork start from zero each
  time it starts, and a reused slot carries nothing from its last user.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/tracer/tracer.zig#L165))
- **Never truncate silently.** Output that does not fit a fixed buffer is an error, not cut short.
  With fixed tables and buffers, a path or name too long for its field is refused with a named
  error.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/src/evm.zig.md?plain=1#L70))
- **A tripped limit says what and why.** When a limit or a validator trips, report the value, the
  limit, the rule that failed and the ids that reproduce it. The named error for a full table
  carries the table, its size and the call that hit it, for the diagnostics on the host. What
  returns to a computer stays as `refusal-says-so` proposes: denied, or not answered.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/internal/safety_counter.zig#L29))
- **Say why beside each limit.** A comment beside each limit and each chosen representation says why
  it has this value or form, and states its unit.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/preprocessor/dispatch_jump_table.zig#L8))
- **A missing limit refuses.** When required limit state is missing, fail; never fall back to a
  permissive default. A user who set no budget is a state of its own, written down explicitly. "No
  rule means deny" already does this for rules.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/BUG_HUNT_REPORT.md?plain=1#L133))
