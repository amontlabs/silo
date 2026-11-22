# Enforcing and recording

What happens to a change of traffic when the record cannot be written is the decision
`change-when-record-cannot-be-written` in the part `traffic`, which follows from this one. The record names no check for this
sentence: `simulator` is put forward here.

## For the owner

An agent's part, 22 November 2026. The part of Silo that writes the record is down during a heavy
build, and a rule refuses a tool call. Three promises in force say that each such decision, refusal
and change to traffic is recorded.

The options:

- Nothing unrecorded (recommended). Silo keeps room for its own decisions. When that room is full, a
  request that needs a new decision is blocked until the record catches up. It costs agents blocked
  while the record is down.
- Carry on, record a gap. Rules keep applying and work goes on. The record shows only that there was
  a gap. It costs the three promises, since some refusals appear nowhere.

Recommended: nothing unrecorded. The record is the core of the product, and a refusal nobody can
find afterwards is worse than a short block.

This also answers `change-when-record-cannot-be-written`, which follows from it. How a request is
blocked without a refusal going unrecorded is for the agents who build it.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Reporting never replaces the outcome.** A failure while reporting does not change the
  operation's own result, and a failed diagnostic does not skip the update it observes.
  `enforcing-and-recording` keeps enforcement independent of the record. This extends it: a failed
  diagnostic never changes or skips the operation it describes.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/test_runner.zig#L111))

## Found by an audit, 22 November 2026

An agent's part. This would set "Enforcing does not depend on recording", with a gap recorded when
the buffer is full. In force, Silo changes nothing in traffic without recording it
(`denied-tool-call`) and each decision is recorded (`how-a-rule-is-enforced`). The recorder is down,
the buffer is full, and a rule denies a tool call: the denial goes ahead and only a gap is recorded.
The audit proposes that this entry needs `change-when-record-cannot-be-written` and says "except
where Silo changes traffic: there the change is recorded or it is not made."
