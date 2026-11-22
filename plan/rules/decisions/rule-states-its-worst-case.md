# What a rule shows when it is set

## For the owner

An agent's part, 22 November 2026. You allow api.github.com on main, which holds your repositories.
Later you set a rule that stops main if a fake secret ever leaves it.

The options:

- Every rule says its worst case (recommended). The first rule says that anything main can read can
  be sent to api.github.com. The second says that a stop ends every program running in main. It
  costs one sentence to read on each rule, and the sentence has to be right: a worst case put too
  mildly is worse than none.
- Only rules that grant say it. An allow says what can leave, and the other rules say nothing. It
  costs a surprise the day a rule you set stops a computer in the middle of a long run.

Recommended: every rule. A precaution already says its worst case when it is set, and a rule can do
more than a precaution.

An agent's part. Until 21 November 2026 the plan asked this twice: here, and as `rule-worst-case`,
whose answer was the first sentence of this one and whose notes read "A proposal the design relies
on, not yet decided." The two were made one, and this one moved from The boundary to this part,
since it is about every rule. The owner let the agent do it and has not ruled on the answer.
