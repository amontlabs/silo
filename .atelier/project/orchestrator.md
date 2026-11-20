## Silo
One session works on one entry of the plan, a decision or a feature, from discussion to done.

Start with `git pull` and `plan`, then `plan show <id>` for the entry at hand: it prints the entry, what it links to and what binds it.

Discussing:
- A proposal is shown to the owner in the conversation first, and written into an entry once the owner has reacted.
- What we agree goes in the entry's file and its notes, not only in the chat.
- Anything about another entry goes in that entry's notes. A new question or idea gets its own entry: a file in its part, and its place in `plan/order.json`.
- A statement the owner has not taken is never written into a list: it sits under `sets` of the entry that proposes it.
- A verdict the owner gives in the conversation: `plan take <id>` or `plan drop <id>`. "Not now" is a move to a later stage of `plan/order.json`.
- Commit the plan files straight to `main` and push, so the Workshop shows it. No branch, review or approve. First run `tools/ci/check.sh plan`, then `links`, then `build` (seconds each; they read `plan/`), and commit only while no merge is running. Plan edits made while building stay on the build's branch.
- A rejected push means the owner decided meanwhile: when no merge is running, `git pull --no-rebase`, then push again. Never force.

Decisions (rule 2b):
- About this entry: its notes file is the decision log.
- Binding every entry: the plan's lists (`words.json`, `principles.json`, each part's `properties.json`) and every taken decision that is not replaced. `plan show <id>` prints the ones that bind an entry. Read them before deciding anything.
- A task, for a worker or a reviewer, names the properties and decisions that apply to it, so that nobody decides again or flags what was decided on purpose.

Building, only a feature the owner took:
1. `plan show <id>`: every entry it needs is done.
2. Build it by the rules above. Each property it adds gets a check, and the property's `at` names that check's file.
3. Write `built` with today's date in the feature's file, in the last branch, when every part is merged and proven. `plan check` refuses it until the feature's needs are done and each property it adds has its `at`. Push.
