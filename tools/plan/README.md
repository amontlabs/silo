# tools/plan

`bin/plan` reads the plan of Silo in [`plan/`](../../plan/), checks it, and records the owner's
verdicts. The plan itself, its files, its keys and its statuses, is described in
[`plan/README.md`](../../plan/README.md); this page does not repeat it.

Node 26, no dependency, no network call. `npm test` runs the tests, which use their own fixtures
(`test/fixtures/sample`, written to a temporary directory) and never `plan/`.

## The commands

| Command | What it does |
|---|---|
| `plan` | Shows the first entry in the order with no verdict (its kind, its question or its name, and `suggest`; for a decision its `answer`, or when it has none the statements it sets, and each decision it names in `replaces` with its `answer`, or the ids it adds when it has none), the features being built, the features ready to build, and the features that wait and for what. |
| `plan show <id>` | For an entry: first its state, then the entry whole (its keys, its `sets` in full, its stage and position, each need with its status, what needs it, its notes), then what binds it, compact: every principle and the properties of the root, of its part and in `keeps` (id and `says`); the taken decisions in force of its part and in `needs` (id and `answer`, or the ids it adds when it has none); the other taken decisions in force of the root as ids only, which `--all` prints in full. A decision in force is taken and not replaced. A replaced decision, one that has `replaced_by`, prints `Replaced by: <id>` first. A need that is replaced prints `replaced by <id>`. Each decision named in `replaces` prints with its `answer`, or the ids it adds. For a part: its title and line (and the word of the same id with its meaning, when there is one), its taken entries in force (and its replaced and dropped ones) as a count and their ids, then each entry with no verdict in the order of `order.json` (id, kind, status, and its `asks` or `name`), the first marked `next`, then its properties (id and `says`). For a statement: the statement, the entry whose `adds` brought it, the features that keep it, and the entries that would replace or remove it. `--json` gives everything, whatever `--all`. |
| `plan take <id> [--by agents]` | The owner's yes. It writes what is listed under the table, in one commit named `Owner: take <id>`. On an entry the agents took, it is the owner's confirmation: it removes `by` and nothing else. With `--by agents` it is the agents' yes: the same write plus `"by": "agents"`, in a commit named `Agents: take <id>`. |
| `plan drop <id> [--by agents]` | The owner's no. Writes `"verdict": "drop"`, keeps `sets`, `removes` and `replaces`, and commits `Owner: drop <id>`. On an entry the agents dropped, it is the owner's confirmation: it removes `by` and nothing else. With `--by agents` it is the agents' no: it also writes `"by": "agents"` and commits `Agents: drop <id>`. On a feature being built it removes `started` in the same write, since only a taken feature holds `started`, and its message says so. |
| `plan check` | Checks the plan. The merge gate and the site's build run it. |

What `plan take <id>` writes:

1. Each statement the entry sets goes into its list. A statement with the same id is replaced.
2. Each statement the entry removes leaves its list.
3. Those ids leave the `adds` of the entries that had them.
4. In the entry, `adds` (the ids it set) takes the place of `sets` and `removes`, and
   `"verdict": "take"` is written, then `"by": "agents"` with `--by agents`. `replaces` stays.
5. Each decision the entry names in `replaces` gets `"replaced_by": "<id>"`.

It commits only the files it changed, and deletes none.

`--by` takes `agents` and nothing else; without it the verdict is the owner's, and a `by` the entry had (from an earlier agents' verdict) is removed. `plan take` of an entry the agents took, and `plan drop` of one they dropped, change only that: the owner's confirmation. A confirmed entry that was replaced stays "already taken". `--by agents` is refused on:

- an entry the owner ruled on: any commit reachable from `HEAD` or the base (`origin/main`, else `HEAD`) wrote it a verdict with no `by`, so an edit that took the verdict away changes nothing. The message names the way: a new decision that names it in `replaces`;
- an entry whose `sets` or `removes` (or `adds`) hold a principle, a property or a word, which are the owner's;
- a verdict the owner wrote (no `by`) that the history does not know, such as one in a plan with no history.

The pure `take` and `drop` need `ownerRuled(id)` in their options with `by: 'agents'`, and refuse without it. `plan --help` lists them. `plan`, `plan show` and `plan check` take `--json`; `plan show` on an entry takes `--all`. Exit codes: 0 done,
1 refused or errors found, 2 wrong use.

`plan take` refuses when the plan fails the check, when `plan/` has uncommitted changes, when the
entry is built, when a decision has neither an `answer` nor a `sets`, or when a decision needs a
decision that is not taken. A decision that puts forward only statements needs no `answer`: its `sets`
stands for one. The take is refused when a decision it names in `replaces` is not taken, or was
already replaced by another decision. It is refused when a decision it names in `replaces` would
keep an `adds`. It is refused when a taken decision it does not name in `replaces` would be left
with neither an `answer` nor an `adds`. A taken decision with an `answer` whose every statement the
take moves stays in force, with no `adds`. On an entry already taken it says so and exits 0. On a
replaced decision it also names the decision that replaced it, from its `replaced_by`. `plan drop` refuses a built entry, a
replaced decision, a taken entry that has `adds`, and a taken decision that has `replaces`. It
refuses a taken decision that a decision with no verdict names in `replaces`, and names that
decision. It also refuses a plan that fails the check or has uncommitted changes. Any other taken
decision may be dropped. A feature being built may be dropped too: `plan drop` removes its `started` with the verdict. Both check the plan they would write and write
nothing when it fails.

Uncommitted changes are asked of git with every untracked and ignored file listed, whatever the
user's settings; only a path the reader skips too (a part of its name starts with a dot) is left
out, and when git cannot answer the verdict is refused. A file under `plan/` that git is told not
to look at (`git update-index --assume-unchanged` or `--skip-worktree`) refuses the verdict too,
and the message names it: git would hide an edit of it. The check before a verdict includes the
rules against the merged plan, except for the entry ruled on. The files are written all or none:
when one cannot be written, or the commit fails, the ones already written are put back.

## What the check refuses

Each problem names the file, the field, what is wrong and the fix.

- A file under `plan/` that is not one of the plan's, except a name that starts with a dot, and a
  part without its `README.md`. A symbolic link anywhere under `plan/`, to a file or a folder,
  whatever its name, a dot-named one too, and a `plan/` that is itself a link: the reader never
  follows one, on disk or at a git ref.
- A file whose bytes are not valid UTF-8. A real U+FFFD character is text like any other.
- JSON that is not in canonical form (`JSON.stringify(value, null, 2)`, a final newline, keys in
  the model's order), an unknown key, a missing required key, or an optional key that is empty.
- An id that does not match `^[a-z0-9][a-z0-9-]{0,62}$`, or that two items of the plan share:
  ids are one namespace across decisions, features, words, principles and properties.
- A part id that is also the id of an entry, a principle or a property, or named `decisions` or
  `features`, and a principle or property in a `sets` named like a part: `plan show` takes a part's
  id as it takes the others. A word may share a part's id, since a part is often named by the word
  for its concept; `plan show` on that id shows the part, with the word's meaning.
- A `verdict` or `suggest` that is not `take` or `drop`; `sets` or `removes` on a taken entry;
  `adds` on an entry that is not taken; a taken decision that is not replaced and has neither an
  `answer` nor an `adds`, or a taken decision that needs a decision not taken.
- A `replaces` that is not a list of decisions, names one twice or names its own entry. A
  `replaces` that names a decision that is not taken, on a decision that is taken or has no
  verdict; a dropped decision's `replaces` is checked for its shape only. A replaced decision that still has `adds`. A chain of `replaces` among taken decisions that
  comes back to its start.
- A `replaced_by` on a decision that is not taken. A `replaced_by` that does not name a taken
  decision whose `replaces` names this decision. A taken decision whose `replaces` names a decision
  that has no `replaced_by` naming it back: so two taken decisions cannot name the same one. On a
  decision with no verdict, or dropped, `replaces` asks nothing of the decisions it names.
- A `sets` item whose id is an entry's, or a statement of another kind. The id of a statement of the
  same kind is allowed: in the same list it replaces it, in another list it moves it.
- An entry whose `sets` and `removes` name the same statement.
- A statement that is not in the `adds` of exactly one taken entry, or an `adds` id that names no
  statement.
- A link (`from`, `keeps`, `needs`, `removes`) to an id that does not exist. The `from` of a
  property an entry sets may name a principle that the same entry sets.
- An entry missing from `order.json` or in it twice, an entry placed before something it needs
  (which also refuses cycles), or an entry that needs a dropped one while not dropped itself.
- A `built` that is not a real calendar day (`YYYY-MM-DD`), or set on a feature that is not taken,
  whose needs are not all done, or whose added properties lack an `at`.
- A `started` that is not a real calendar day, or set on a feature that is not taken, whose needs
  are not all done, or whose `built` is an earlier day. A `built` needs no `started`.
- A path of an `at` that is not relative to the top of the repository (absolute, with an empty,
  `.` or `..` part), or that does not name a regular file there: a directory, a missing file, or a
  link that leads outside the repository. Without the repository, only the file test is left out,
  and a note says so.
- Against the merged plan (`origin/main`, else `HEAD`; only the command checks this, not the
  offline check of a tree):
  - an entry whose question, answer, name, description, `replaces`, `sets` or `removes` changed
    while its verdict stayed;
  - an entry whose verdict is `take` or `drop` and is not the one the merged plan holds for it
    (another verdict, none, or no such entry: a verdict typed in, or an entry copied from a decided
    one). A verdict taken away by hand passes, and so does a plan whose merged plan holds no entry
    (the remote's old plan, until the first push).

  Neither applies to an entry whose file holds what the owner ruled on. The evidence is the latest
  commit since the base, on any merged branch, named `Owner: take <id>` or `Owner: drop <id>`, or
  `Agents: take <id>` or `Agents: drop <id>` (for a verdict typed in, the one of its kind). The
  commit's file and its subject agree on who wrote the verdict: `Agents:` with `"by": "agents"`,
  `Owner:` with no `by`. A `by` added or removed by hand while the verdict stayed is refused too. An entry with `"by": "agents"` is refused when the history of `HEAD` or the base ever gave it a verdict with no `by` (`ownerRuledBefore`), whatever the commit subjects say, and when it sets, removes or adds statements. Of two such commits, one made on top of the other is
  the later, whatever their dates. That commit must have written the entry's file, found in it by its id
  wherever the file was then. The entry's file now, committed or not, must agree with that version
  on the verdict and on the keys above. A merge, a move, a change of other keys (`adds`,
  `replaced_by`, `suggest`, as a take of another entry or a suggestion does) or a change that was
  put back come between without effect; a later ruling replaces an earlier one. The check does not
  catch `plan take` run without the owner's word, a commit named `Owner: take <id>` or
  `Agents: take <id>` written by hand, or an owner's commit amended or squashed with another change.

A malformed value is a problem like any other: the check never throws on one.

These are notes, not errors: a property without `from`; a decision with no verdict whose
`replaces` names a decision already replaced; an entry not done and not dropped that needs a
replaced decision.

## The modules

| Folder | What |
|---|---|
| `model/` | Names, key orders, the canonical form and the statuses. Imports no `node:` module, so the site can use it. |
| `read/` | Reads `plan/` from a directory or a git ref into one plan object. |
| `check/` | `checkFiles(files, { exists })` for a tree, `checkChange(files, baseFiles)` against the merged plan, `formatProblem`. |
| `write/` | `take(files, id)` and `drop(files, id)`: pure functions, files in and changed files out, so the Workshop can call them. |
| `history/` | `readSteps(root)` reads from git the commits that changed an entry, and `entryHistory(steps)` turns them into each entry's changes, and `ownerRuled(steps)` into the ids of the entries that held a verdict with no `by` in any step. `readSteps(root, ref)` reads the history as of `ref` (`HEAD` by default). None is a command. `entryHistory` and the names it uses import no `node:` module, so the site can use them. |
| `cli/` | The command: arguments, the checkout, git. |
| `bin/plan` | The entry point. |

## The history of an entry

`readSteps(root)` returns `{ steps, shallow }` for the git repository at `root`. It starts at most three git processes, whatever the number of commits, and makes no network call. It never throws: with no repository, no commit or no entry in the history, `steps` is empty.

A step is a commit of the first-parent chain of `HEAD` that changed an entry's file or its notes. A merge is one step, compared with its first parent. `shallow` is true for a shallow clone, and then the oldest step holds the whole plan as added.

`entryHistory(steps)` returns a `Map` from an entry's id to its changes, newest first. An entry is followed by the name of its file (`decisions/<id>.json`, `<part>/decisions/<id>.json` or `<part>/features/<id>.json`), wherever the file is, and its notes are the `<id>.md` beside it. Any other path is ignored.

A change is `{ commit, date, what }`, with `text` or `by` when `what` names them, and `agents: true` when it is `taken` or `dropped` and the verdict has `"by": "agents"`. `commit` is the full hash and `date` the committer date in UTC. `what` lists one or more of these names, in this order, and `WHAT` exports them:

| Name | The step |
|---|---|
| `added` | is the first where the entry exists, or where its file comes back after a deletion. It names nothing else. |
| `moved` | puts the file under another part, or the root. |
| `text` | changes `asks` or `answer` of a decision, `name` or `what` of a feature. `text` is `{ before: { title, body }, after: { title, body } }`, with `body` null when there is none. |
| `sets` | changes `sets`, `removes` or `replaces` of an entry with no verdict before and after it. |
| `needs`, `keeps`, `surfaces`, `suggest` | changes that key. |
| `notes` | changes the notes, adds them or deletes them. |
| `taken`, `dropped` | gives the entry that verdict, or the same verdict from another hand: the owner confirming what the agents took or dropped, or the agents writing again over the owner's, is a `taken` or `dropped` too. |
| `cleared` | takes the verdict `take` or `drop` away. |
| `replaced` | adds `replaced_by`. `by` is its value. |
| `started` | adds `started`. |
| `built` | adds `built`. |

A step that changes none of these gives no change: `adds`, `suggested_in`, the order of keys and white space are not changes, because values are compared as parsed JSON. A step where the entry's file is not a JSON object is skipped for that entry, and the next step is compared with the last version that parsed. A step that deletes the file gives no change.
