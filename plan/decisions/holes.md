# Holes

Settled by polarzero after 19 November 2026.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Tools that read never fail on bad data.** Analysis and display accept malformed or truncated
  input and report it. Corruption is reported, never silently repaired. The record reader shows a
  damaged stretch as a gap and resumes only at the next boundary it can verify, as `holes` asks:
  what is missing stays missing.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/src/bytecode/README.md?plain=1#L111))
