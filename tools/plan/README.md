# tools/plan

`bin/plan` reads the plan of Silo in [`plan/`](../../plan/), checks it, and records the owner's
verdicts. The plan itself, its files, its keys and its statuses, is described in
[`plan/README.md`](../../plan/README.md); this page does not repeat it.

Node 26, no dependency, no network call. `npm test` runs the tests, which use their own fixtures
(`test/fixtures/sample`, written to a temporary directory) and never `plan/`.

## The commands

| Command | What it does |
|---|---|
| `plan` | Shows the first entry in the order with no verdict (its kind, its question or its name, and `suggest`; for a decision its `answer`, or when it has none the statements it sets, and each decision it names in `replaces` with its `answer`, or the ids it adds when it has none), the features ready to build, and the features that wait and for what. |
| `plan show <id>` | For an entry: first the entry whole (its keys, its `sets` in full, its stage and position, each need with its status, what needs it, its notes), then what binds it, compact: every principle and the properties of the root, of its part and in `keeps` (id and `says`); the taken decisions in force of its part and in `needs` (id and `answer`, or the ids it adds when it has none); the other taken decisions in force of the root as ids only, which `--all` prints in full. A decision in force is taken and not replaced. A replaced decision, one that has `replaced_by`, prints `Replaced by: <id>` first. A need that is replaced prints `replaced by <id>`. Each decision named in `replaces` prints with its `answer`, or the ids it adds. For a part: its title and line (and the word of the same id with its meaning, when there is one), its taken entries in force (and its replaced and dropped ones) as a count and their ids, then each entry with no verdict in the order of `order.json` (id, kind, status, and its `asks` or `name`), the first marked `next`, then its properties (id and `says`). For a statement: the statement, the entry whose `adds` brought it, the features that keep it, and the entries that would replace or remove it. `--json` gives everything, whatever `--all`. |
| `plan take <id>` | The owner's yes. Writes what the entry sets into the lists, removes what it removes, moves those ids out of the `adds` of the entries that brought them, writes `adds` and `"verdict": "take"`, keeps `replaces`, writes `"replaced_by": "<id>"` in each decision the entry names in `replaces`, and commits only the changed files as `Owner: take <id>`. It deletes no file. |
| `plan drop <id>` | The owner's no. Writes `"verdict": "drop"`, keeps `sets`, `removes` and `replaces`, and commits `Owner: drop <id>`. |
| `plan check` | Checks the plan. The merge gate and the site's build run it. |

`plan --help` lists them. `plan`, `plan show` and `plan check` take `--json`; `plan show` on an entry takes `--all`. Exit codes: 0 done,
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
replaced decision, a taken entry that has `adds`, and a taken decision that has `replaces`. It also
refuses a plan that fails the check or has uncommitted changes. A taken decision that brought no
statement and replaces nothing may be dropped. Both check the plan they would write and write
nothing when it fails.

Uncommitted changes are asked of git with every untracked and ignored file listed, whatever the
user's settings; only a path the reader skips too (a part of its name starts with a dot) is left
out, and when git cannot answer the verdict is refused. A file under `plan/` that git is told not
to look at (`git update-index --assume-unchanged` or `--skip-worktree`) refuses the verdict too,
and the message names it: git would hide an edit of it. The check before a verdict includes the
rule against the merged plan, except for the entry ruled on. The files are written all or none:
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
  `replaces` that names a decision that is not taken, whatever the verdict of the entry that names
  it. A replaced decision that still has `adds`. A chain of `replaces` among taken decisions that
  comes back to its start.
- A `replaced_by` on a decision that is not taken. A `replaced_by` that does not name a taken
  decision whose `replaces` names this decision. A taken decision whose `replaces` names a decision
  that has no `replaced_by` naming it back: so two taken decisions cannot name the same one. On a
  decision with no verdict, or dropped, `replaces` asks nothing of the decisions it names.
- A `sets` item whose id is an entry's, or a statement of another kind. The id of a statement of the
  same kind is allowed: in the same list it replaces it, in another list it moves it.
- A statement that is not in the `adds` of exactly one taken entry, or an `adds` id that names no
  statement.
- A link (`from`, `keeps`, `needs`, `removes`) to an id that does not exist. The `from` of a
  property an entry sets may name a principle that the same entry sets.
- An entry missing from `order.json` or in it twice, an entry placed before something it needs
  (which also refuses cycles), or an entry that needs a dropped one while not dropped itself.
- A `built` that is not a real calendar day (`YYYY-MM-DD`), or set on a feature that is not taken,
  whose needs are not all done, or whose added properties lack an `at`.
- A path of an `at` that is not relative to the top of the repository (absolute, with an empty,
  `.` or `..` part), or that does not name a regular file there: a directory, a missing file, or a
  link that leads outside the repository. Without the repository, only the file test is left out,
  and a note says so.
- A word listed in a word's `not`, used anywhere else in `plan/`.
- Against the merged plan (`origin/main`, else `HEAD`): an entry whose question, answer, name,
  description, `replaces`, `sets` or `removes` changed while its verdict stayed. Only the command
  checks this, not the offline check of a tree. It does not apply to an entry when the latest
  commit since the base that changed its verdict, question, answer, name, description, `replaces`,
  `sets` or `removes` is the owner's verdict on it (`Owner: take <id>` or `Owner: drop <id>`), and
  none of those has changed since: the owner ruled again after the change. A commit that changed
  only its `adds` or its `replaced_by`, as a take of another entry does, does not count. A change of
  `replaced_by` alone is never refused.

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
| `cli/` | The command: arguments, the checkout, git. |
| `bin/plan` | The entry point. |
