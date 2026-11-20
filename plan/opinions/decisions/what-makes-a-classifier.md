# What makes a plugin a classifier

## The owner's words

- First, as a hunch: "classifier" may be only a name for a plugin whose output fits, not a strict
  kind.
- On 20 November 2026 an answer was put forward that called it a role. The owner first found it
  very clear, then was no longer sure: "Do you think it's accurate?"
- On 21 November 2026, on the answer above: "much better". The owner asks whether a more accurate
  word exists for what produces an opinion, and wants to see every kind of output a plugin has
  before the word is settled.

## An agent's notes

- "Role" was too vague: it did not say what makes something a classifier. The declaration does.
- It needs `shipped-plugins-declare`: if what ships with Silo had no declaration, a built-in
  classifier would not be a plugin.
- The plan used "classifier" for two things: the program someone adds, and what gives an opinion.
  The first is now "plugin": `plugin-is-the-subject` in the part `plugins`.
- The word was compared with estimator, detector, monitor, evaluator, reviewer and labeler. None
  is both standard and plainer. "Classifier" is exact for an opinion that is a kind from a fixed
  list. For the other shapes the plan calls an opinion, see `opinion-is-a-value`.

## The owner's answer (21 November 2026, later)

"Classifier" is the correct word, and the reason is that it produces a value from a fixed list,
more than that it produces an opinion. It is one specific case, not a generic category, and the
owner wonders why the design fixates on it before the outputs of a plugin are known. The answer
above is worded from this.
