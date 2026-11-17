# tools/plan

The plan of Silo lives in [`plan/`](../../plan/) as plain files. Read them directly, and edit a
feature in its own files. `bin/plan` does what touches GitHub or more than one file.
Node 26, no dependency. `npm test` runs the tests, which use their own fixtures and a fake GitHub
(`test/fake-github.js`), never `plan/`, the real GitHub or the real Keychain.

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
state could not be read (GitHub, or the credentials to reach it). Run from anywhere inside the checkout.
Every command that reads the plan first runs the checks of `plan check`; when the plan fails them, it
prints its errors and exits 1.

| Command | What |
|---|---|
| `plan` | The commands, one line each. |
| `plan status` | The latest verdicts (the newest five commits touching `plan/features/` whose subject starts with `Owner: `), the next feature to decide (the first in order without a verdict), what is being built, what is taken and not started (ready, or waiting for which needs), and ideas not placed; with live state, the features whose verdict label on GitHub differs from the file, under `Not in sync`, one line each saying what each side has. |
| `plan show <id>` | One feature: its fields, stage and position, each need with its status, what needs it, the decisions about it (those whose `about` names its area, and those with no `about`), its notes, and its issue when live, with a `Not in sync` line when its verdict label differs from the file. |
| `plan add <id> --name "…" --area <area> (--after <id> \| --first-in <stage>) --what "…" [--from <issue>]` | A new feature file and its place in `order.json`. `--from` sets `suggested_in`. Prints the paths; commits nothing. Never into the stage `gone`. Refuses an id that has a file or is already in `order.json`, and writes nothing unless the plan it leaves passes `plan check`. |
| `plan finish <id>` | Writes `built` (today, UTC) in a feature whose verdict is take and whose needs are built, and prints the `git commit` line, with the path as seen from the directory it was run in. The next sync closes the issue and removes the building label. |
| `plan check` | The checks below. Each error reads `file: field: message. Fix: …`. |
| `plan decide <id> take\|later\|drop\|clear` | A verdict the owner gave in conversation. The bot puts the label on the issue (the new one before the old one comes off) and comments `Recorded for the owner from a conversation.` (`clear` removes every verdict label and comments `Verdict cleared for the owner from a conversation.`). Prints `Recorded on GitHub. The commit follows: git pull.` When GitHub shows that verdict already: if the file at `origin/main` (fetched) has it too, prints `Already recorded: …` and changes nothing; else the bot removes the label and adds it again (for `clear`, adds the file's verdict label and removes it), so the newest verdict event is the bot's and a run starts, and comments as usual. Never writes the file. Exit 3 without credentials or GitHub; refused on a built feature. |
| `plan start <id> [--branch <b>]` | Refused unless the file's verdict is take and every need is built. Live: refused when `workshop:building` is set (naming the branch of the newest claim); else sets it, comments `Being built on <branch>.` (the checkout's branch), then reads the issue again. The earliest claim posted since the label was set wins, by comment id, whatever its branch: when it is not this session's, this session lost the race, deletes its comment and exits 1. Without GitHub, or without an issue yet: says the claim is not shared, exit 0. |
| `plan stop <id>` | Removes `workshop:building`. On a feature not being built it says so, exit 0. |
| `plan decline <issue> --why "…"` | Closes an open suggestion as not planned, the reason as the bot's comment. |
| `plan sync [--dry-run]` | The plan to GitHub (below). Run by `plan.yml`; by hand when it failed. |
| `plan pull [--write \| --commit]` | The owner's verdicts from GitHub to the files (below). Run by `plan.yml`. |
| `plan setup [--org] [--site] [--replace]` | Creates the App (below). |
| `plan store [--replace]` | Finishes a setup whose Keychain step failed, from `vercel.env`. |
| `plan doctor` | Where each credential comes from (never a value), whether the App is public and installed, its permissions, the six labels. Exit 1 on a problem. |

Live state (the issues on GitHub) is read through `live/`, in this order: the site's
`GET <site>/api/workshop/state` (one call, no token; `issue?n=` for one issue's comments; the site
is `https://silo.amontlabs.com`, `PLAN_SITE` overrides it), else GitHub with the bot's token, else
"not available". `status` and `show` then say so for their live parts and print the rest; they exit
0 when GitHub is not configured (no credentials), 3 when it was tried and failed. The verdict they
show is always the file's: an agent acts on its checkout. Live state adds only what is being built,
the issue, its votes and comments.

## GitHub

Labels: `workshop:entry`, `workshop:suggestion`, `verdict:take`, `verdict:later`, `verdict:drop`,
`workshop:building`. Only these three are verdict labels, everywhere (`verdict:refine` is none), and
label names read from GitHub are compared ignoring capitalisation. An entry's issue is one the bot
created whose body ends with `<!-- silo-entry: <id> -->`, found among every issue the bot created
(`GET /issues?creator=<bot>`), not by its `workshop:entry` label; when two carry one marker the
oldest is used. Its body also
holds `<!-- silo-synced: <commit> -->`, the commit it was last synced at. The bot is recognised by
its login, `<app slug>[bot]`, or by `performed_via_github_app` naming the App on a bot's item.

Repo to GitHub, `plan sync`: runs `git fetch origin main` and reads the plan at `origin/main`
(refused when it fails `plan check`). It creates missing labels; creates, updates, closes (as
completed when `built`, as not planned when the verdict is drop) and reopens each entry's issue, and
puts `workshop:entry` back on one that lost it; removes `workshop:building` from a closed one;
closes the open issue of a feature no longer in the plan as not planned, commenting `No longer in
the plan.`; closes an open duplicate (the higher number of two bot issues with one marker) as not
planned, commenting `Duplicate of #N.`, unless it carries a verdict label (then it is reported and
left open); closes a suggestion named by `suggested_in` once, commenting `Placed in the plan as #N:
<link>`. When a feature's files changed between its issue's marker and `origin/main`, it posts one
history comment (no text from the plan) and moves the marker; when the marker's commit is not an
ancestor any more, it only moves the marker. A feature unchanged since its marker keeps it. A marker
is never moved backwards: when `origin/main` is an ancestor of the marker's commit (fetched first
when it is not here), that issue is left alone, so a run of an older commit beside a newer one
changes nothing the newer did. When the bot already posted a history comment whose `before` is the
marker's commit and whose `commit` is an ancestor of `origin/main` (a run stopped between the comment
and the marker), sync starts from that comment's `commit`; with nothing changed since, it only moves
the marker. A verdict label that pull has not brought to the file yet holds the issue's state. Sync
never writes a verdict label. Writes go one a second, so a run stopped half way (exit 3 on GitHub's
rate limit) resumes without duplicates, and a second run changes nothing.

GitHub to repo, `plan pull`, the only path from a label to a file: one verdict label counts when
its newest `labeled` event is by the bot or by a person with write, maintain or admin permission
(`GET /collaborators/<login>/permission`, its `role_name` or `permission`); the file gets it. No
verdict label: the file's verdict is removed only when the newest verdict event is a removal that
counts the same way. A file verdict whose issue has no verdict event at all is reported (`the file
says take, GitHub has no verdict: was the issue replaced?`) and left alone. A label that does not
count, two verdict labels, a built feature whose issue shows another verdict: reported, the file
left alone. A file changed
during the run is left alone. `--write` edits the files; `--commit` also commits them, subject
`Owner: take <id>, drop <id>` (72 characters at most, then `…`), a line per feature with who and when.

The workflow, `.github/workflows/plan.yml`, does both directions in one job, so that whichever run
survives in the queue does everything. Triggers: a push to `main`, an `issues` `labeled` or
`unlabeled` event with one of the three verdict labels, `workflow_dispatch`. Concurrency group
`plan`, never cancelled, set on the job so a skipped job never joins it. Steps: check out `main`
with its whole history and no stored credentials; `plan pull --commit`; `plan check`; `bin/push-main`;
`plan sync`, which runs even when the pull step failed. The workflow's own token (`contents: write`)
is given to the push step only, as `PUSH_TOKEN`. `bin/push-main` pushes `HEAD` when it is a new
commit and never rebases: on a rejected push it fetches `main`; if `main` did not move it fails at
once with git's own message (a protected branch or a bad token is not "main moved"); else it resets
to `origin/main`, runs `plan pull --commit` and `plan check` again and pushes; three tries. A
commit that fails the check is never pushed. The pull, push and sync steps use the secrets
`WORKSHOP_APP_ID` and `WORKSHOP_PRIVATE_KEY`.

The App: `plan setup` opens a page on `127.0.0.1` (it answers no other Host) that posts the
manifest to GitHub: "Silo Workshop", public, Issues write and Metadata read, no webhook, sign-in
callback `<site>/api/workshop/callback`. The owner clicks "Create GitHub App", then "Install" on
`amontlabs/silo` only. The command writes the seven variables Vercel needs to
`~/.local/state/silo-workshop/vercel.env` (0600) first, then the App id, client id, client secret
and private key to the Keychain (service `silo-workshop`, through `security -i` on stdin). It never
registers a second App by accident; `--replace` registers a new one. Credentials are looked up in
the environment, then the Keychain (a locked one stops the command), then
`~/.config/silo-workshop/credentials.json` (0600 or refused). `WORKSHOP_GITHUB_API` and
`WORKSHOP_REPO` override the API and the repository.

Not verified until the owner has created the App: the workflow, and every rule above that rests on
how GitHub answers (the bot's login and `performed_via_github_app`, listing issues by
`creator=<slug>[bot]`, label events and their actors, label names matched ignoring capitalisation,
the permission endpoint for an App token, closing and reopening with a reason, fetching a commit by
its id, `contains` in the workflow's `if` ignoring case). The tests hold them
against the fake. No credential, token or key is ever printed or logged.

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
- `github/`: the REST client, the App's JWT and installation token, credentials, the Keychain,
  `setup` and `store`, `doctor`, `sync`, `pull`, and `decide`, `start`, `stop`, `decline`.
- `live/`: the live state, from the site or GitHub; `cli/`: the commands.
