# Rules that allow, hold or deny a request

## The choice

An agent's part, 22 November 2026. Main has three rules: allow github.com, allow crates.io, hold
every push. A program in main connects to paste.rs. No rule allows it, so it is denied, and the
record keeps the decision with the version of the rules that made it.

The options:

- Build it (recommended). Each request is allowed, held or denied, the strictest rule wins, and the
  order you wrote the rules in never matters. It costs the first hours with a computer: no rule
  means deny, so it reaches nothing until you allow hosts or open its network.
- Leave it out. A computer's network is on or off and nothing finer. It costs holds, asks, limits on
  secrets and every rule Silo could propose, which all need it.

Recommended: build it. A hostile agent can then do what the rules allow and nothing else, which is
what Silo says about itself.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

It is large: it holds what decides on a request, rules kept as data, and the command line.
