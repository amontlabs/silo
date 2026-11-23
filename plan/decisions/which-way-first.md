# When several ways can do the same thing, which one Silo uses

## The choice

An agent's part, 23 November 2026. You want main never to push to `main` on GitHub. A classifier
that judges each tool call could deny `git push origin main`. A network rule on main's requests to
GitHub can deny the push itself.

The options:

- A principle (recommended). Every design starts from the most reliable way, and goes down only
  when that way cannot do it. Here the push is denied by a network rule. A classifier is kept for
  what no rule can see, such as a command that deletes files inside main. It costs one more
  question that every addition answers.
- Case by case. It costs the cases where nobody asks: a classifier built where a rule would have
  been exact, which an agent can fool, and on which nothing can be promised.

Recommended: a principle. It is polarzero's direction, and it is how the plan already treats facts
and reports (`facts-before-reports`).

## polarzero's direction (23 November 2026)

In an agent's words.

- Whenever a design reaches for a precaution, or for an extension, first ask whether a rule can do
  it, and whether there is a simpler design.
- This is not "rules only". Silo goes from the most secure and most reliable way to the less
  reliable ones, and at each step asks whether the thing can be done in a better way.
- Classifying tool calls to stop a push to GitHub, or to handle secrets, would be bad design: a
  network rule and a secret swapped in at the boundary do it exactly.

## Notes

- `facts-before-reports`, which is in force, is this principle applied to what Silo knows. It
  stays, because it answers a narrower question exactly.
- `rules-are-the-promise`, still to decide, says what a rule and a precaution are worth. This
  principle says which one a design reaches for first.
- The case that led to it: denying a tool call is a precaution (`rule-on-a-tool-calls-text`), used
  only for what no rule can see.
