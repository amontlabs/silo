# How something is checked when its right answer is an opinion

The record goes on: "The law in 5.1 limits what can act on a classifier's opinion. Silo checks
that limit and scores the opinion separately: what can grant or destroy is checked hardest, and
opinions were made unable to grant or destroy anything so that they do not need proof." The law
is in the part `rules`.

| What | Is there an exact statement of "right"? | How it is checked |
|---|---|---|
| Recording facts | Yes: everything that crossed the boundary has a fact, or a gap | Hostile tests on the real boundary, the balance of the record, the simulator with faulty sources |
| The core's decisions: access, rules that deny or hold | Yes: only the user, or a rule the user set, can grant. A matching deny beats a hold, a matching hold beats an allow, and a request without a grant is denied | The model of access, the simulator, proof later if needed |
| What a precaution may do | Yes: only protective actions | A precaution's type has no other action in it; the simulator |

The rows on a classifier's mechanics and a classifier's opinion are in the part `opinions`, the row
on an added classifier in the part `extensions`.
