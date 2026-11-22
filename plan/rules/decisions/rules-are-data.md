# Whether setting a rule or a precaution requires writing code

## The choice

An agent's part, 22 November 2026. You want main to deny packages younger than 7 days.

The options:

- Rules are data (recommended). You pick the rule and type 7, in the app or as a line of text on the
  command line. It costs reach: a rule Silo does not offer is one you cannot write, until Silo or an
  extension adds it.
- Rules are code. You write a script or a policy in a language, and it can say anything. It costs
  what Silo can tell you about it: Silo can no longer state a rule's worst case or check that the
  order of rules never matters, and a mistake in your code is a hole.

Recommended: rules are data. The policy languages that last are the ones a tool can read and reason
about, and a rule you can set in one step is one you actually set.

## Notes

A proposal the design relies on, not yet decided.
