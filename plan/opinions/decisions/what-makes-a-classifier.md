# What makes an extension a classifier

## The owner's words

- First, as a hunch: "classifier" may be only a name for an added program whose output fits, not
  a strict kind.
- On 21 November 2026: "classifier" is the correct word, and the reason is that it produces a
  value from a fixed list, more than that it produces an opinion. It is one specific case, not a
  generic category. The owner wondered why the design fixated on it.

## An agent's notes

- Why the design fixated on it: it started from one law, that what can be wrong may only lead to
  protective actions. The law needed a name for what gives the value, and the whole part was named
  after that one case.
- Where the case shows: a request waits at the boundary, and the extension answers from a list
  Silo fixes (`what-wakes-an-extension` in the part `extensions`).
- It needs `built-in-classifiers-run-inside`, which says whether a built-in classifier is an
  extension.
- The word was compared with estimator, detector, monitor, evaluator, reviewer and labeler. None
  is both standard and plainer.
- `what-a-classifier-is` still says a classifier is whatever gives an opinion. `opinion-is-a-value`
  notes the three shapes the plan calls an opinion.

## Still unclear to the owner (21 November 2026)

"I still don't know what the output is, like how it works."

An agent's note. The only output an extension has is its answer about one waiting request: one of
three words that Silo fixes (`what-wakes-an-extension`). Everything else goes through tools.
Nothing in the part `extensions` needs the word "classifier", so this decision can wait, and the
fifth film can show the case without the word.

## The owner's answer (21 November 2026, later)

The owner now understands it this way: an answer belongs to a tool. An extension that has the tool
"deny or hold the network requests of main" must give that answer, and other tools may bring other
answers. It can be called a classifier if it really needs a name. The owner agrees to leave this
decision undecided and to keep the word out of the fifth film.

## Found by an audit, 22 November 2026

An agent's part. `meanings-for-what-silo-knows` sets `classifier` as "Whatever gives an opinion",
and this entry as "An extension that gives a value from a fixed list": the second take overwrites
the first. The built-in classifier for heat is a function that `processes` runs in the recorder, not
an extension with a computer. The audit proposes that this entry alone sets the word: "Whatever
gives a value from a fixed list as its opinion: a pattern or a function inside Silo, or an
extension." A second audit asks instead: is each built-in an extension with a computer?

The question is put to the owner in `built-in-classifiers-run-inside`, under "For the owner".
