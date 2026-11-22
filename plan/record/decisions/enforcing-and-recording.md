# Enforcing and recording

What happens to a change of traffic when the record cannot be written is the open decision
`change-when-record-cannot-be-written` in the part `traffic`. The record names no check for this
sentence: `simulator` is put forward here.

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
