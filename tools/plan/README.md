# tools/plan

`bin/plan` reads the plan of Silo in [`plan/`](../../plan/), checks it, and records the owner's
verdicts. The plan itself, its files, its keys and its statuses, is described in
[`plan/README.md`](../../plan/README.md); this page does not repeat it.

Node 26, no dependency, no network call. `npm test` runs the tests, which use their own fixtures
(`test/fixtures/sample`, written to a temporary directory) and never `plan/`.

## The commands

| Command | What it does |
|---|---|
| `plan` | Shows the first entry in the order with no verdict (its kind, its question or its name, and `suggest`), the features ready to build, and the features that wait and for what. |
| `plan show <id>` | For an entry: its keys, its stage and position, each need with its status, what needs it, its notes, and what binds it: every principle, the properties of the root, of its part and in `keeps`, and the taken decisions of the root, of its part and in `needs`. For a sentence: the sentence, the entry whose `adds` brought it, the features that keep it, and the entries that would replace or remove it. |
| `plan take <id>` | The owner's yes. Writes what the entry sets into the lists, removes what it removes, moves those ids out of the `adds` of the entries that brought them, writes `adds` and `"verdict": "take"`, and commits only the changed files as `Owner: take <id>`. |
| `plan drop <id>` | The owner's no. Writes `"verdict": "drop"`, keeps `sets` and `removes`, and commits `Owner: drop <id>`. |
| `plan check` | Checks the plan. The merge gate and the site's build run it. |

`plan --help` lists them. `plan`, `plan show` and `plan check` take `--json`. Exit codes: 0 done,
1 refused or errors found, 2 wrong use.

`plan take` refuses when the plan fails the check, when `plan/` has uncommitted changes, when the
entry is built, when a decision has no `answer`, or when a decision needs a decision that is not
taken. On an entry already taken it says so and exits 0. `plan drop` refuses a built entry and a
taken entry that has `adds`, and also refuses a plan that fails the check or has uncommitted
changes. Both check the plan they would write and write nothing when it fails.

## What the check refuses

Each problem names the file, the field, what is wrong and the fix.

- A file under `plan/` that is not one of the plan's, except a name that starts with a dot, and a
  part without its `README.md`.
- JSON that is not in canonical form (`JSON.stringify(value, null, 2)`, a final newline, keys in
  the model's order), an unknown key, a missing required key, or an optional key that is empty.
- An id that does not match `^[a-z0-9][a-z0-9-]{0,62}$`, or that two items of the plan share:
  ids are one namespace across decisions, features, words, principles and properties.
- A `verdict` or `suggest` that is not `take` or `drop`; `sets` or `removes` on a taken entry;
  `adds` on an entry that is not taken.
- A sentence that is not in the `adds` of exactly one taken entry, or an `adds` id that names no
  sentence.
- A link (`from`, `keeps`, `needs`, `removes`) to an id that does not exist.
- An entry missing from `order.json` or in it twice, an entry placed before something it needs
  (which also refuses cycles), or an entry that needs a dropped one while not dropped itself.
- A `built` that is not a date, or set on a feature that is not taken, whose needs are not all
  done, or whose added properties lack an `at` naming files that exist.
- A word listed in a word's `not`, used anywhere else in `plan/`.
- Against the merged plan (`origin/main`, else `HEAD`): an entry whose question, answer, name,
  description, `sets` or `removes` changed while its verdict stayed. Only the command checks this,
  not the offline check of a tree.

A property without `from` is a note, not an error.

## The modules

| Folder | What |
|---|---|
| `model/` | Names, key orders, the canonical form and the statuses. Imports no `node:` module, so the site can use it. |
| `read/` | Reads `plan/` from a directory or a git ref into one plan object. |
| `check/` | `checkFiles(files, { exists })` for a tree, `checkChange(files, baseFiles)` against the merged plan, `formatProblem`. |
| `write/` | `take(files, id)` and `drop(files, id)`: pure functions, files in and changed files out, so the Workshop can call them. |
| `cli/` | The command: arguments, the checkout, git. |
| `bin/plan` | The entry point. |
