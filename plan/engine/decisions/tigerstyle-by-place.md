# Which rules of TigerStyle hold where

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

- `engine-shape` takes TigerStyle "in full in the core, where it fits in drivers" without saying
  which rules. This entry names them.
- Guillotine's maintainer calls its style "housecat": TigerStyle, skipping the rules you do not
  like. Its code shows where that leads: rules that are only written down become 26 `catch {}`.
  Hence one list per directory, enforced by the check, not by taste.
- The quotas are left out because an agent meets a quota with assertions that cannot fail, which
  `check-must-catch-a-fault` then has to find. This is the one point put to polarzero.
- Tracer hooks and a ring of recent operations, which the maintainer recommends, belong in the
  helper and the drivers. The core needs none: its inputs are already the trace, and they replay.
