# Can an input make the engine stop

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

- An assertion stops the engine (`assertions-stay-on`), and the engine then restarts from saved
  state. If something a computer sends could break an assertion, an agent could stop the engine
  for every computer, and stop it again after each restart.
- Guillotine's agent file holds the same rule: a crash that input can trigger is a missing
  validation, and the validation is fixed first
  ([CLAUDE.md](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/CLAUDE.md?plain=1#L21)).
- The simulator sends hostile values in every field a computer controls, and checks that no
  assertion fails.
- An accepted call is carried out or fails with a reason (`accepted-call-never-forgotten`). An
  input replayed after a restart could stop the engine again, so the one that stopped it is set
  aside, and fails with that reason.
