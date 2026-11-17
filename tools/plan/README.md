# tools/plan

The plan of Silo lives in [`plan/`](../../plan/) as plain files. Read them directly, and edit a
feature in its own files. `bin/plan` does what touches more than one file (and, later, GitHub).
Node 26, no dependency. `npm test` runs the tests, which use their own fixtures and never `plan/`.

## The data

All JSON is UTF-8, written as `JSON.stringify(value, null, 2)` plus a final newline, keys in the
order below. Unknown keys are errors. An optional key is absent, never `null`.

| File | Holds |
|---|---|
| `PRODUCT.md` | What Silo is, in prose. |
| `areas.json` | `[{ "id", "title", "line" }]`: the parts of the product. |
| `order.json` | `{ "stages": [{ "id", "title", "line", "features": [ids] }] }`: the build order. Every feature appears exactly once. The first four stages are the starter. The stage `gone`, when present, is last: old things proposed to be left behind. |
| `decisions.json` | `[{ "id", "date", "text", "about" }]`: what the owner settled for the whole product. `about` (optional) lists area ids, each once; absent means the whole product. A decision about one feature goes in that feature's notes. |
| `features/<id>.json` | One feature; the file name is its id, `^[a-z0-9][a-z0-9-]{0,62}$`. |
| `features/<id>.md` | Optional notes: what was agreed, open points, and `## From the old code`. Its decision log. |

A file or folder whose name starts with a dot (`.DS_Store`) is ignored. Any other file is an error.

A feature's keys, in order:

| Key | |
|---|---|
| `name`, `what` | Required, non-empty. |
| `area` | Required, an id of `areas.json`; only a feature of the stage `gone` may have none. |
| `needs` | Required, ids of other features (may be `[]`); no cycle. |
| `surfaces` | Optional `{ "app", "cli" }`, each optional. |
| `suggest` | Optional `{ "verdict", "why" }`: what was proposed to the owner. |
| `suggested_in` | Optional: the number of the visitor's suggestion issue it came from. |
| `verdict` | Optional: `take`, `later` or `drop`; absent means undecided. Never written by hand. |
| `built` | Optional: `YYYY-MM-DD` (UTC), only with `"verdict": "take"`. Written by `plan finish`. |

## Status

From the feature file and, when known, its issue. The verdict that counts is the issue's verdict
label when it has one, else the file's. First match wins:

| When | Status |
|---|---|
| `built` is set | `built` |
| verdict drop | `dropped` |
| verdict take and the issue has `workshop:building` | `building` |
| verdict take | `planned` |
| verdict later | `later` |
| otherwise | `undecided` |

Ready to build: `planned`, and every feature in `needs` is `built`.

## Commands

Every command takes `--json`, before or after the command name. Exit codes: 0 done, 1 refused or errors found, 2 wrong use, 3 live
state could not be read. Run from anywhere inside the checkout. `status`, `show`, `add` and `finish`
first run the checks of `plan check`; when the plan fails them, they print its errors and exit 1.

| Command | What |
|---|---|
| `plan` | The commands, one line each. |
| `plan status` | The latest verdicts (the newest five commits touching `plan/features/` whose subject starts with `Owner: `), the next feature to decide (the first in order without a verdict), what is being built, what is taken and not started (ready, or waiting for which needs), and ideas not placed. |
| `plan show <id>` | One feature: its fields, stage and position, each need with its status, what needs it, the decisions about it (those whose `about` names its area, and those with no `about`), its notes, and its issue when live. |
| `plan add <id> --name "…" --area <area> (--after <id> \| --first-in <stage>) --what "…" [--from <issue>]` | A new feature file and its place in `order.json`. `--from` sets `suggested_in`. Prints the paths; commits nothing. Never into the stage `gone`. Refuses an id that has a file or is already in `order.json`, and writes nothing unless the plan it leaves passes `plan check`. |
| `plan finish <id>` | Writes `built` (today, UTC) in a feature whose verdict is take and whose needs are built, and prints the `git commit` line, with the path as seen from the directory it was run in. |
| `plan check` | The checks below. Each error reads `file: field: message. Fix: …`. |

Live state (the issues on GitHub) is read through `live/`. For now it is never available: `status`
and `show` say so for their live parts and print the rest.

Coming with the GitHub side: `plan decide`, `start`, `stop`, `decline`, `sync`, `pull`, `setup`,
`store`, `doctor`.

## `plan check`

Offline. It fails when:

- a file is missing (`PRODUCT.md`, `areas.json`, `order.json`, `decisions.json`), is not JSON, or
  is not a file of the plan;
- a file does not match its shape: an unknown key, a `null`, a missing or empty required text, an
  invalid id, an id twice;
- a feature is not in `order.json`, is there more than once, or `order.json` names an id with no file;
- the stage `gone` is not last;
- `area` is missing outside `gone`, or names no area;
- `needs` names a missing feature, the feature itself, a feature twice, or makes a cycle;
- `suggest.verdict` or `verdict` is not take, later or drop; `suggested_in` is not an issue number;
- `built` is not a date, or is set without the verdict take;
- a decision's id is not unique, its date is not a date, or its `about` is empty, names something other than an area (a feature id included), or names an area twice;
- an `.md` has no `.json`;
- a JSON file is not in canonical form.

When `areas.json` has an area that the merged plan (`origin/main`, else `HEAD`) does not, it says
so: the area's place on the map is drawn by hand in `website/src/workshop/map.ts`, and the site
build checks it.

## Modules

- `model/`: names, the status function, area states and counts, the three GitHub markers
  (`silo-entry`, `silo-synced`, `silo-change`) and the wanted issue of an entry, the canonical
  form. Plain ESM; it imports nothing outside its folder and no `node:` module (a test enforces it).
- `read/`: `readPlan(dir)` and `readPlanAt(repo, ref)` (`repo`: any directory inside the repository) give one plan object: `areas`, `stages`
  (with their feature ids in order), `features` by id (each with `id`, `stage`, `position` from 1,
  and `notes`), `decisions`.
- `check/`: `checkFiles(files)` gives `{ errors, notes }`.
- `live/`: the live state interface; `cli/`: the commands.
