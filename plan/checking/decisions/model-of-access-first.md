# Is a model of access written before the first engine code

- The one property of the model is, in force, `grant-needs-a-fact-or-you`, in the part `rules`.
- Which tool checks the model is `model-tool`.
- "The starter" is the first engine code.

## A model in another language drifts

An agent's challenge, 21 November 2026. A model written in a language of its own is checked on its
own terms: when the code gains a retry path that the model does not have, the checker stays green
and says nothing about the code. Two texts make the case: Joseph Hellerstein, "No Room for Bugs",
13 October 2026, and Andrew Helwer, "The changing role of finite-state model checking", 24 August
2026. Both were read in full.

The answer above already holds the remedy: the simulator compares the Zig core with the model. So
the tool must be able to exchange histories with the simulator, and that became a criterion of
`model-tool`. The owner agreed on 21 November 2026.

Still open: once the core exists, the real core can be run over every sequence of calls up to a
size, because it is a pure function over small tables. The model is then needed only before the
code exists. The trial of `model-tool` has this as one of its candidates.
