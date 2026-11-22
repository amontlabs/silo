# An extension that protects

## The choice

An agent's part, 22 November 2026. Push check is installed on main with the right to hold its
pushes. An agent pushes to the main branch of amontlabs/site. The push waits, Silo asks Push check,
and it answers "hold": you get an ask that shows its opinion. Had it answered "let it go on", your
rules would have decided. Had it not answered in time, the push would have been denied or left to
your rules, as you chose when installing it.

The options:

- Build it (recommended). An extension can stop what it finds risky before it happens, and never let
  through what your rules deny. It costs time on each request it covers: a check that asks a model
  can take seconds, so it suits pushes and deploys more than every download. A wrong "deny" also
  costs a retry.
- Leave it out. Extensions watch and alert after the fact. It costs every check that should come
  before a push or a deploy goes out, unless you write a rule for it yourself.

Recommended: build it. A push about to leave is where a judgement is worth the wait, and since an
extension can only deny, hold or pause, a wrong answer costs time and never opens anything.
