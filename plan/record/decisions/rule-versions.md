# Rule versions

## The choice

An agent's part, 22 November 2026. At 15:00 you tighten a rule on main so that it reaches github.com
only. At 15:00:01 a connection to npmjs.org still goes through.

The options:

- Rules have versions (recommended). The line of that connection names the version of the rule that
  let it pass: the old one, a second before the new one arrived. It costs keeping every past version
  of your rules.
- The latest rule only. The line names the rule, not which text of it. It costs the answer to why
  that connection passed.

Recommended: rules have versions. A record of decisions is of little use if you cannot tell which
rule text decided.

## Notes

"Driver" and "core" are the engine's words, set by `engine-shape` in the part `engine`. The record
names no check for this sentence: `simulator` is put forward here.
