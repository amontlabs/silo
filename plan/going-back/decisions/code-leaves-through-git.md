## Found by an audit, 22 November 2026

An agent's part. This says code leaves a fork through Git at any time, and `what-a-fork-is-given`
gives a fork "read-only repository access, unless you, or a rule you set, give it more". An agent
finishes a fix in a fork and runs `git push`: by that default the push is denied, so the code leaves
after a grant. The audit proposes to keep the safer default and to add to `what-a-fork-is-given`:
"Its commits leave when you grant the push or bring them to the parent (`keeping-from-a-fork`)."
This decision then reads: Git is the way out, open at any moment of the fork's life.

The question is put to the owner in `what-a-fork-is-given`, under "The choice".
