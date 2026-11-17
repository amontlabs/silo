## Silo
One session works on one feature, from discussion to built.

Start with `git pull` and `plan status`, then `plan show <id>` for the feature at hand.

Discussing:
- What we agree goes in the feature's two files, not only in the chat.
- Anything about another feature goes in that feature's notes. A new idea gets its own entry with `plan add`.
- A verdict the owner gives in the conversation: `plan decide <id> take|later|drop`.
- Merge and push, so the Workshop shows it.
- A rejected push means the owner decided meanwhile: when no merge is running, `git pull --no-rebase`, then push again. Never force.

Decisions (rule 2b):
- About this feature: its notes file is the decision log.
- Binding every feature: `plan/decisions.json`, what the owner settled for the whole product. Read it before deciding anything. Delete an entry once it no longer changes what an agent does.
- A task, for a worker or a reviewer, names the decisions that apply to it, so that nobody decides again or flags what was decided on purpose.

Building, only a feature the owner took:
1. `plan start <id>`. It refuses if another session has the feature.
2. Build it by the rules above.
3. `plan finish <id>` in the last branch, when every part is merged and proven. Push.
