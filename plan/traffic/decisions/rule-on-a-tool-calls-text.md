# Can a rule deny a tool call by its text

## The choice

An agent's part, 23 November 2026. You want main never to run `rm -rf ~/projects/site`, a command
that acts only inside main. The model asks for it.

The options:

- No: a precaution denies it (recommended). An extension that protects judges the call, with a
  model or with a pattern on its text, and denies it. The probe makes the call fail inside main,
  and nothing in the traffic changes (`denied-tool-call-mechanism`). It costs a promise: an agent
  can get around a precaution, by writing the command another way, or through the probe, which
  runs inside main.
- Yes: a rule with a pattern on the text, as proposed on 22 November. It costs one of two things.
  Either the rule is enforced inside main, by the probe, against `where-a-rule-is-enforced`. Or
  Silo changes the reply and every later request, which Anthropic's terms may forbid on a Max
  plan. Either way, `git push -f` still gets past a pattern on `git push --force`.

Recommended: no. A rule is exact, and a denial carried out inside main is not. What a rule can see
exactly is denied by a rule: a push to GitHub is a request that a network rule denies.

## polarzero's direction (23 November 2026)

In an agent's words.

- Denying a tool call becomes a precaution rather than a rule, and it always should have been one.
  The classifier is a precaution anyway, since nobody can be sure it works well. As soon as one
  part is a precaution, the whole thing is a precaution.
- A pattern on the text is only one example of an extension that protects.
- A push to GitHub belongs to the rules on GitHub, and secrets are swapped in at the boundary.
  Classifying those at the tool call would be the wrong design: more complicated, and less sure.
  The principle behind this is put forward in `which-way-first`.
