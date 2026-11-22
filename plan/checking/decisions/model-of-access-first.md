# Is a model of access written before the first engine code

Until 21 November 2026 the answer proposed a small model in a language of its own, checked by a
model checker before the first engine code, with the simulator then comparing the Zig core with
it. Which tool would check it was `model-tool`. The first property to check is, in force,
`grant-needs-a-fact-or-you`, in the part `rules`.

## A model in another language drifts

An agent's challenge, 21 November 2026. A model written in a language of its own is checked on its
own terms: when the code gains a retry path that the model does not have, the checker stays green
and says nothing about the code. Two texts make the case: Joseph Hellerstein, "No Room for Bugs",
13 October 2026, and Andrew Helwer, "The changing role of finite-state model checking", 24 August
2026. Both were read in full.

The answer of that day held a remedy: the simulator compares the Zig core with the model. So the
tool had to be able to exchange histories with the simulator, and that became a criterion of
`model-tool`. The owner agreed on 21 November 2026.

## No model in another language

The trial of `model-tool` then ran a small real core in Zig over every sequence of calls at the
model's bounds. It took 44 s, and no model in another language was quicker to check. Its numbers
are in the notes of `model-tool`.

The owner, 21 November 2026, asked whether to drop the separate checking language: "Yes we go full
zig." And on why: "it's our language, it's great that we can test with it, and we'll really use
the language to prove, conceptualize, visualize". The answer was reworded on that day to say so.

The owner then asked why not use the simulator alone. The reasons are in the notes of
`four-ways-to-check`. The owner, 21 November 2026: "we always use simulator and when we have a reason to believe
certain order we do that in addition". The properties of access depend on the order
of calls, as `rule-order-never-matters` says outright, so both are used here. The owner took the
entry on 21 November 2026.

## What the plan does not settle for access

The trial had to assume these to write the model. Each is a question for its own entry.

- `actions-a-rule-offers` leaves open which actions a rule offers. Assumed: any.
- A hold ends at Silo's limit (`hold-limit`, taken). The trial assumed that nothing ends a hold, because it was written when `how-a-hold-ends` was open. That entry now asks only how the refusal is delivered. When access is built, a hold ends at the limit.
- What a rule matches is not settled (`rules-are-data`, `allow-hold-or-deny`). Assumed: one
  computer and one host, exactly.
- Taking a rule away is not designed. Once it exists, three of the four properties fail as the
  trial worded them, because the rules a request saw are read against the rules of now. The
  owner, 21 November 2026: a rule can of course be removed, and this is written when that part is
  planned.
- What a drop does to the computer's secrets is not settled. Assumed: nothing.
- `importing-changes-nothing`, in the part `extensions`, is also checked this way. The trial left
  it out.

## A small version first

The owner, 21 November 2026, when taking the entry: "what I'm planning for I think is to always start with a poc, like
something small with relevant code, structures, and some test so I can read what the flow is,
discuss it, then implement in the actual code."

The owner added the same day that this has not started yet. It is how the owner plans to work, and
no part has been built this way so far.
