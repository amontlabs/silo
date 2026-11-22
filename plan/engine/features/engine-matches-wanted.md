# Computers brought to what you asked

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **A kill that fails is escalated.** When stopping a process fails, record that it may still be
  running and escalate, instead of only warning. Silo never shows a computer as stopped while its
  virtual machine may still run.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/test_runner.zig.md?plain=1#L459))
