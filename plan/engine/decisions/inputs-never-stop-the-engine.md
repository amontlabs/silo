# Can an input make the engine stop

## The choice

An agent's part, 22 November 2026. An agent in a fork sends Silo a malformed message, by accident or
on purpose, and it reaches a line of the engine that nobody expected it to reach.

The options:

- A bad input is refused (recommended). Everything that can be sent to the engine is checked on the
  way in, and what fails the check gets an error. The engine stops itself only on its own bugs,
  never on what it was sent. If a bug still lets one through and the engine stops, that message is
  set aside after the restart and its sender gets a failure with the reason. It costs a check on
  every field of every message, and a test that sends hostile values to all of them.
- Stop on anything unexpected. Simpler to build, and every surprise shows at once. It costs every
  computer its changes each time, since one agent can stop the engine for all of them, and again
  after each restart.

Recommended: a bad input is refused. An agent is treated as hostile, so nothing it can send may stop
Silo for the other computers.

## Notes

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
