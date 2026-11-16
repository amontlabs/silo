# The plan as data

How every fact of Silo's plan is stored, changed, checked and read. For the agents who build this and for every agent who later uses it. Repository: `amontlabs/silo`. Folder: `plan/`, in the current tree and in the restarted tree. Words used throughout: the Workshop is the public page `https://silo.amontlabs.com/workshop` (`website/src/workshop/`) and its server (`website/server/`, endpoints under `/api/workshop/`, described in `docs/WORKSHOP.md`); the bot is the GitHub App that the tool and the workflow write as; the gate is `atelier merge`; the orchestrator is the agent that plans, merges and talks to the owner; a worker builds one item in its own worktree.

Five points wait for the owner. This document proceeds on the recommended answer and marks each place that depends on one. (1) A verdict or answer the owner gives in conversation counts as soon as the agent records it; the owner's page lists it with one click to undo, and nothing quotes the owner in public. (2) A taken feature whose text changes stays taken and is listed for the owner as changed; one that absorbs a feature the owner dropped goes back to undecided. (3) Agents push `main` whenever the gate is green, once the Workshop is live. (4) Work that is not a product feature is on the plan, listed apart, never drawn on the public map. (5) A feature is built only when every surface it lists exists.

## 1. The rule
Every fact has one home and one kind of writer.

| Kind | Facts | Home | Changed by |
|---|---|---|---|
| Defined | what a feature is, its rules, what it needs, the order, the areas, decisions, whether this tree contains the feature | files under `plan/` | a commit that passes the merge gate (section 10) |
| Recorded | the owner's verdict, being built now, votes, comments, suggestions, questions and answers | GitHub issues | the Workshop server or the `plan` tool, nothing else |
| Derived | status, area state, counts, positions, sentences, pages, an issue's title, body and open or closed state | nowhere | computed by `tools/plan/model/`; never stored, never edited by hand |

- Nothing is copied between homes. A file that must act on something recorded holds a reference to an event that already happened (`receipt.ref`, `answers`, `suggested_in`). A past event cannot go stale; a live check verifies each reference.
- Versions: the plan exists as a worktree, as local `main` and as `origin/main`. Issues mirror `origin/main`, nothing else. Every command prints the version it read.
- The repository holds no verdict. The gate never reads GitHub: a check whose answer changes without a commit is flaky by agent rule 3.

## 2. Homes
`#` is the row in section 1 of `inventory.md`; `+` marks a fact that had no home. "Orchestrator" means a commit on a plan branch (section 10) unless a command is named. A decider is a person whose repository role is write, maintain or admin. "The owner" in this document means any decider: whoever the owner gave that role decides in the owner's name.

| # | Fact | Home | Writer | Read by |
|---|---|---|---|---|
| 1 | Feature id | the file name `plan/features/<id>.json` | orchestrator (`plan new`) | everything |
| 2, 3 | Name, sentence | `name`, `what` in the feature file | orchestrator | page, tool; `plan sync` writes them as issue title and body |
| 4, 5 | Build order, stage of a feature | the position of the id in `plan/order.json` | orchestrator (`plan move`) | `model` |
| 6, 7, 8, 10, + | Area, needs, suggestion and reason, surfaces, rules | `area`, `needs`, `suggest`, `surfaces`, `rules` in the feature file | orchestrator | page, tool, `model`, checks |
| 9 | What the old code had | `plan/legacy/<id>.json` | orchestrator | `plan legacy <id>` only; `plan show` prints its `lessons` |
| 11, + | The owner's verdict and the text it covers | label `verdict:<value>` and the `silo-verdict` mark (with `digest`) in the issue body | a decider through `POST verdict`; orchestrator through `plan decide` | server, then page and tool through `GET state` |
| 12 | Who gave it, when | GitHub's `labeled` event on the issue | GitHub | `plan show`, live checks |
| 13 | The note | the comment marked `silo-note` | the writer of the verdict | page, `plan show` |
| + | That a build started from a taken text | `receipt` in the feature file | `plan start` only | `plan check`, live checks |
| 14 | Being built now | label `workshop:building` and the bot comment marked `silo-build` | `plan start`, `plan stop`; `plan sync` removes the label once built | server, page, tool |
| 15, + | Built, and when; surfaces that exist before that | `built` (a date); `done` | worker, in the code's merge | `model`; `plan sync` closes the issue |
| 16 | Recordings, tests | `recordings`, `tests`, and the files they name | worker | page, checks |
| 17 | The issue of a feature | the `silo-entry` marker in the body of an issue the bot created | `plan sync`; a record command on demand | server, tool. Never a key in a file |
| 18, 19 | Votes, comments | reactions and comments on the issue | people | page, `plan show` |
| 20, + | Suggestions, and what one became | issues labelled `workshop:suggestion`; then `suggested_in` on the feature, or `workshop:seen`, or closed | people; `plan new --from`, `plan triage` | page, `plan status`; `plan sync` closes it with a link |
| 21 | A question, its default, what it is about | an issue labelled `workshop:decision` with a `silo-question` mark | `plan ask` | page (deciders), tool |
| + | The answer | the closing comment of the question (section 6) | a decider through `POST decide`; orchestrator through `plan answer` | decision record (`quote`), live checks |
| 22 | A decision | `plan/decisions/<date>-<slug>.json` | orchestrator (`plan decision`) | page (deciders only), tool. Not mirrored |
| 23, 24, 31 | Stage titles and lines, the starter boundary; area titles and lines | `stages[]` and `starter_through` in `plan/order.json`; `plan/areas.json` | orchestrator | page, home page |
| 25, 26, 27 | Area state, status, counts, numbers, generated sentences | nowhere: `model` | none | page, home page, tool |
| 28 | What Silo is | `plan/PRODUCT.md`. The public pitch is a different fact: `website/index.html`. `README.md` keeps one line and a link | orchestrator; site worker | agents; visitors |
| 29 | Architecture | decided: a decision record, long form in `docs/design/<topic>.md` named by its `doc` key. Proposed: a question with a default | orchestrator | agents, owner |
| 30, + | Where the work stands; what the orchestrator has already seen; work that is not a feature | nowhere: `plan status` prints obligations that stay true until met, so there is no cursor. The work is a feature whose area has `"product": false` (owner question 4; the recommendation stands until the owner answers) | orchestrator | tool; the page lists it apart |
| 32 | Who may decide | GitHub repository role; the list of deciding roles is in `model` | repository settings | server, live checks |
| 33 | Label, marker and mark names | `tools/plan/model/names.js` | tool worker | server, page, tool |
| + | Per-area presentation (map layout, glyphs) | page code keyed by area id; area ids are permanent and the site build fails on a key that is not an area | page worker | page |
| + | Key descriptions | `plan/schema/*.schema.json` | tool worker | checker, guide |
| + | The tool's own writes of the last ten minutes | `~/.local/state/silo-plan/recent-writes.json`, a bounded cache (section 7) | the tool's write commands | the tool's read commands only |

Removed. Keys: `n`, `stage`, `verdict`, `issue`, `why` (now `suggest.why`), `app`, `cli` (now `surfaces`), `old`, `old_lines`, `reuse` (now in the legacy record). Files: everything under `docs/product/` (`features.jsonl`, `stages.json`, `areas.json`, `DECISIONS.md`, `old-code-map.jsonl`, `RESTART.md`; `PRODUCT.md` moves to `plan/`), `tools/workshop/` (becomes `tools/plan/`), and after go-live `tools/dashboard/` with its store. Concepts: a verdict kept in a file, `workshop pull`, the mirror of decided lines as closed issues, the stage `gone`, the constants `living` and `gone`, the `last-pull` cursor, a feature kind for leftovers, the dashboard's notes on product sections (its `look` and `decide` items are questions; `shipped` is `built` plus `recordings`).

## 3. Files
The files: `plan/PRODUCT.md`, `plan/order.json`, `plan/areas.json`, `plan/features/<id>.json` with an optional `plan/features/<id>.md`, `plan/decisions/<date>-<slug>.json`, `plan/legacy/<id>.json`, `plan/schema/<kind>.schema.json` (feature, order, areas, decision, legacy, carried-verdicts), and `plan/carried-verdicts.json`, which exists only from the cutover to go-live (section 12). `plan/PRODUCT.md` says what Silo is, in prose: it holds no count of features, entries or stages and no list of stages or areas. All JSON is UTF-8 in canonical form: `JSON.stringify(value, null, 2)` with a final newline, so every key and list item has its own line. Keys follow the schema's order; an object keyed by ids (`plan/areas.json`, `plan/carried-verdicts.json`) keeps the order it has, and `plan format` sorts nothing there. Optional keys are absent, never `null`. Unknown keys are an error. Dates are UTC. The examples below are compacted.

**A feature**, `plan/features/fork.json`. `name`, `what`, `area`, `surfaces` and `suggest` are today's values; `needs` and `rules` are drafts; the last two lines show what a build adds.
```json
{ "name": "Fork a computer",
  "what": "Copy a computer, or one of its checkpoints, into a new independent computer for a task, and throw it away when done.",
  "area": "forks", "needs": ["checkpoints"],
  "rules": [ { "id": "cold-boot", "text": "A fork starts from the disks, never from memory, so nothing of the source keeps running." },
             { "id": "own-identity", "text": "A fork gets its own Silo-tools token and endpoint, never the ones of its source." } ],
  "surfaces": { "app": "button:Fork", "cli": "silo fork · drop" }, "suggest": { "verdict": "take", "why": "The heart of the product. Proven: 21 s from inside main." },
  "receipt": { "ref": "https://api.github.com/repos/amontlabs/silo/issues/events/15520947731", "digest": "1:573a8a06ef00fb26" },
  "tests": ["engine/sim/fork.zig"], "recordings": ["fork.mp4"], "built": "2026-12-01" }
```
| Key | Value | Rule |
|---|---|---|
| `name`, `what` | text, required | `name`: 1 to 80 characters, unique among features; the issue title. `what`: 1 to 300 characters; the issue body. |
| `area` | area id, required | |
| `needs` | list of feature ids | Absent means none. No cycle. A need has no `merged_into`, and comes earlier when both are in the same list of `plan/order.json`. |
| `rules` | list of `{ "id", "text" }` | What the owner is bound by and what tests name: anything the owner must agree to is a rule, not prose. A rule id is unique in the feature, never changes and is never reused. A test that proves a rule has `<feature id>/<rule id>` in its name. |
| `surfaces` | `{ "app"?, "cli"? }` | Where a person meets it. An absent key means "none, or not said yet". |
| `suggest`, `suggested_in` | `{ "verdict", "why" }`; issue number | The orchestrator's suggestion and its reason. The suggestion issue this feature came from. |
| `merged_into` | feature id | This feature was absorbed. It leaves `plan/order.json`; the file stays. The target has no `merged_into`. |
| `receipt` | `{ "ref", "digest" }` | Proof that the build started from a taken text. Only `plan start` writes it (section 4). |
| `done` | list of keys of `surfaces` | Surfaces that exist while the feature is not built (owner question 5; the recommendation stands until the owner answers). `[]` means "started, nothing usable yet". Removed when `built` is set. |
| `tests`, `recordings` | lists | `tests`: repository paths that exist; for work that is not a feature, a document or a script counts. `recordings`: names of files in `website/public/workshop/recordings/`, ending `.mp4`, `.webm`, `.mov` or `.m4v`, at most 10 MB each. |
| `built` | `YYYY-MM-DD` | This tree contains the feature. Needs `receipt`, every need built, every surface existing, and one entry in `tests` or `recordings`. |

- There is no `id` key: the id is the file name. A feature file is never deleted or renamed. A feature leaves the plan by a drop verdict or by `merged_into`.
- **Its prose**, `plan/features/<id>.md`, optional. Two headings, each optional, in this order: `## Behaviour` (how it works, edge cases) and `## Notes` (for agents). Any other heading, or text before the first heading, is an error. A verdict does not cover prose. Once the feature is built, `## Behaviour` is a record of the plan, not a description of the code. A refinement that fits no key goes under `## Notes`; it becomes a key when a tool must read it (one commit: schema, checker, files).
- **The order**, `plan/order.json`. Titles and lines are today's; the stage list and the feature lists are cut here. A stage has `id`, `title`, `line` and `features`, all required.
```json
{ "starter_through": "living",
  "stages": [ { "id": "shell", "title": "Shell", "line": "One computer you can open from the app and the command line.",
                "features": ["engine-service", "calls-and-events", "vm-runtime", "guest-image", "computers"] },
              { "id": "loop", "title": "The loop", "line": "Checkpoint, fork, see what changed, merge or drop.",
                "features": ["checkpoints", "fork", "quiet-forks", "fork-tree", "diff", "merge", "undo"] } ],
  "work": ["new-tree", "shell-design"] }
```
- Every feature without `merged_into` is listed exactly once: in a stage when its area is a product area, under `work` otherwise. The position of a product feature is its index in the flattened stages, from 1; the position of a work feature is its index in `work`, from 1. `needs` may cross the two lists, which is how a work feature comes before a product feature. The starter is every stage up to and including `starter_through`. Only the orchestrator edits this file.
- **The areas**, `plan/areas.json`: an object keyed by area id. Every area holds at least one feature. A product area's number is its rank by the position of its first feature (1, 2, 3 …). `"product": false` marks an area that is never drawn on the map.
```json
{ "forks": { "title": "Forks", "line": "A copy of a computer for each task." },
  "work": { "title": "Around the product", "line": "The Workshop, the plan and the tools.", "product": false } }
```
- **Work that is not a feature** (owner question 4; the recommendation stands until the owner answers) has the shape of a feature: `plan/features/new-tree.json` with `"area": "work"`, listed under `work`. It has an issue, a verdict, a receipt and `built` like any feature. It is never counted in a public number.
- **A decision**, `plan/decisions/2026-11-16-zig-engine.json`. The id is the file name; its first ten characters are the date, so there is no `date` key. A decision file is never deleted or renamed; a new record replaces it.
```json
{ "who": "owner",
  "text": "The engine is written in Zig. It is the core piece: performant, secure, easy to reason about. The app, the command line and every tool only talk to it. Its design is data first, with events.",
  "about": { "features": ["engine-service", "calls-and-events"], "areas": ["engine"] } }
```
- Keys: `who` (free text) and `text`, required; `text` says what the decision means for the plan and holds no Markdown link. `about`: `{ "features"?, "areas"?, "legacy"? }`, lists of ids; the key types the reference, so ids are unique per kind, not across kinds (`desktop` is a feature, an area and a stage today). `answers`: the number of the question it answers. `quote`: the text of that question's closing comment (section 6), verbatim, cut at 5,000 characters; required with `answers` unless `by_default`. `recorded`: `true` when that comment was written by the bot from a conversation, so `quote` is the agent's sentence and not the owner's words. `by_default`: `true` when nobody answered and the question's default was applied; needs `answers`, has no `quote`. `replaces`: list of decision ids. `doc`: repository path of a design document that exists.
- **A legacy record**, `plan/legacy/linux-app.json`: today's line of `docs/product/old-code-map.jsonl` plus `old`, `old_lines`, `reuse` from the feature list (lists cut here).
```json
{ "carried": false, "name": "The Silo app on Linux", "what": "A Linux build of the desktop app, with packages and an updater.",
  "why": "Dropped on 4 Nov. Linux gets the engine and the command line.",
  "old": "dropped", "old_lines": 4000, "reuse": "rewrite", "tag": "legacy-2026-11-16",
  "evidence": ["app/SiloUI/src-tauri/src/linux_menu.rs"], "calls": [],
  "lessons": ["Many Linux-only bugs came from the AppImage environment (editor and terminal launch)."],
  "sources": ["app-linux-app", "plan-linux-app", "plat-linux-app-packages"] }
```
- `"carried": true` means a feature file has this id. `"carried": false` means none has, and `name`, `what`, `why` are required: the page's "Not carried over" list is these records. `old_lines: 0` means "none, or not counted". `evidence` are paths in the tree at `tag`. `sources` name rows of the raw sweep files on the owner's Mac (`~/.local/state/silo-dashboard/sweep/`), which stay. A legacy record is never deleted. The word has one other, unrelated use: `carried` as the source of a verdict taken over from the old feature list at go-live.
- **Carried verdicts**, `plan/carried-verdicts.json`: an object keyed by feature id; a value is a verdict, or `{ "verdict", "note" }`. Example: `{ "engine-service": "take", "editor": { "verdict": "refine", "note": "…" } }`.
- **The schema subset.** Schema files are JSON Schema 2020-12 limited to `type`, `properties`, `required`, `additionalProperties` (`false`, or one schema for a keyed object), `items`, `enum`, `pattern`, `minLength`, `maxLength`, `minItems`, `uniqueItems`, `description`. The tool's own checker reads them, with no dependency, and refuses any other keyword. The guide prints each `description`. Rules across files are code in `check`.
- **Ids.** Feature, area, stage and rule ids match `^[a-z0-9][a-z0-9-]{0,39}$`. Decision ids match `^\d{4}-\d{2}-\d{2}-[a-z0-9][a-z0-9-]{0,59}$`. Feature, area, rule and decision ids never change. A stage id may change in one commit.

## 4. The verdict
**Where it lives.** On the feature's issue, as two things: exactly one label `verdict:<value>`, and one line in the issue body, the verdict mark:
```
<!-- silo-verdict: {"verdict":"take","digest":"1:573a8a06ef00fb26","source":"click","commit":"<40 hex characters>"} -->
```
`source` is `click` (a decider's own account, on the Workshop), `recorded` (the bot, `plan decide`; adds `"session"`) or `carried` (the bot, `plan carry`). `commit` is optional: the commit whose text was shown. An optional `"decision"` names a decision record. A note is a separate comment by the same writer, ending with the line `<!-- silo-note -->`. Who and when are GitHub's `labeled` event; nothing copies them. Notes, build comments and answers stay comments: the tool reads them from GitHub; the page holds only the newest 1,500 comments of the repository and can lose an old note.
- **Write order**, one routine in `model` that returns the calls; the server and the tool both execute it. To set or change a verdict: add the new label; remove every other verdict label; set the issue's open or closed state (section 5); read the issue again; write its body with the new mark, last. To clear: remove the mark first, then the label. So no writer of ours leaves a mark without its label, and a run cut in the middle leaves a label without its mark, which does not count. Between the calls readers see no verdict: for seconds, and on other server instances for up to about 40 seconds. A Confirm that meets a `plan sync` body write can be lost: the verdict still counts and the feature is listed as changed again. Harmless, not detected.
- **A mark without a label** (a label removed in GitHub's interface) is no verdict. `GET state` gives `unlabelled`, `verdict_unlabelled` lists it, and `plan decide <id> clear` or the next `plan sync` removes the mark.
- **The digest** says which text a verdict covers. Inputs, exactly: `name`, `what`, every rule's `id` and `text`, and the ids of the features whose `merged_into` is this feature. Not prose, area, order, needs, surfaces or suggestion.
```
digest = "1:" + first 16 hex characters of SHA-256( UTF-8( JSON.stringify(
  [ nfc(name), nfc(what), [ [rule.id, nfc(rule.text)] for rules sorted by id ], [ merged ids, sorted ] ] ) ) )
```
`nfc` is Unicode normalization form C; sorting is by code unit; no `rules` is `[]`. The prefix `1:` names this recipe. The digest is stored in two places only: the verdict mark and `receipt.digest`. Only `model` computes it. The value above is the real digest of the example feature of section 3.
- **When a verdict counts.** One function in `model`, from a GitHub issue to its `GET state` record (section 7), decides for the server, the tool and the tests. All three must hold; otherwise the record has `verdict: null` with `foreign` (condition 1) or `unmarked` (the label's value, or `several`), and every reader shows `undecided`:
1. the issue was created by the bot and carries the `silo-entry` marker;
2. it has exactly one `verdict:*` label;
3. its body has one verdict mark naming the same value.
- **Who can make a verdict count.** Conditions 2 and 3 need a writer who can both label and edit the body. GitHub gives both to the roles Moderate, Write, Maintain and Admin; to the bot, that is any holder of the App's credentials (the tool on the owner's Mac, the workflow); to any other App installed with the Issues write permission; and to a workflow token with `issues: write`. The owner grants every one of these. Read and Triage cannot edit a body (Triage can label and close), so GitHub's label menu alone never makes a verdict count. Among them the Workshop takes a click only from a decider, and `plan decide` is the orchestrator's. The backstop is live: `verdict_actor` reads the newest `labeled` event of the verdict label and reports an actor that is neither a decider nor the bot, with its login, its role, and the App when `performed_via_github_app` names one.
- **Recorded from conversation** (owner question 1; the recommendation stands until the owner answers). `plan decide <id> take|refine|later|drop|clear [--note "<text>"] [--decision <id>]` counts at once. Its note comment says "Recorded from a conversation with the owner." and never quotes the owner; `--note` is required for `refine`. The owner's page lists it under "Recorded for you" with Confirm and Undo. Undo clears the verdict: the earlier one is stored nowhere. Confirm is `POST verdict` with the same value: the mark's `source` becomes `click`; the label and its event stay, so `plan show` still names the bot and the first time. One decision that gives several verdicts is one decision record plus one `plan decide --decision` per feature. A verdict is never also a decision record.
- **The receipt.** `plan start <id>` runs in the build branch's worktree and reads the issue from GitHub. It refuses unless the feature is buildable (section 5) and its status is `planned`, or `building` for this same branch; so it refuses when `workshop:building` is set for another branch. Then it writes `"receipt": { "ref": <API URL of the newest labeled event that set verdict:take>, "digest": <digest of the text in this worktree> }` into the feature file, replacing any earlier receipt, commits that file, adds the label, and posts the comment `Being built on <branch>.` ending `<!-- silo-build: {"branch":"…","session":"…"} -->`. GitHub documents no conditional write on an issue: two sessions can both pass the label test within seconds; the second comment shows it and `building_stale` lists it.
  - Offline, in the gate: `built` needs `receipt`; `receipt.ref` is an event URL of this repository; in a diff that adds `built` or changes `done`, `receipt.digest` equals the current digest; a tree to merge that holds `receipt` holds `built` or `done`.
  - Live: `receipt.ref` is a `labeled` event of `verdict:take` on this feature's issue, by a decider or the bot (an event GitHub no longer returns fails the same way), and the verdict is still take. If it is not, the owner changed their mind after the build started: an obligation, not drift.
- **Built in parts** (owner question 5; the recommendation stands until the owner answers). A merge that leaves the feature unfinished sets `done`. The orchestrator then runs `plan stop <id>`: the status is `planned` and the page adds what is done. The branch that continues starts with `plan start <id>`. A reword in between needs nothing more: the receipt's digest is compared only in a diff that adds `built` or changes `done`.
- **Changes after a verdict** (owner question 2; the recommendation stands until the owner answers). The status result carries `changed: true` when a verdict's digest differs from the current one.
  - Reword, new or changed rules: the feature stays taken and building continues. The owner's page lists it under "Changed since you took it"; Confirm writes the new digest.
  - Before and after: the page reads `plan/features/<id>.json` at the mark's `commit` from `raw.githubusercontent.com` (public, readable across origins) and shows that `name`, `what` and `rules` beside the current ones; absorbed features are listed from the current tree. Without `commit` it shows the current text and "The earlier text is not available". With a `commit` that GitHub does not have (a verdict recorded for a text that is not pushed yet) it does not list the feature as changed; it says "Taken for a text that is not published yet". The site build embeds its own commit (`VERCEL_GIT_COMMIT_SHA`, else `git rev-parse HEAD`); the page sends it with a verdict.
  - In a checkout, `plan show` prints a differing digest as `changed` only when the mark's `commit` is an ancestor of `HEAD`; otherwise as `checkout_behind`: this checkout is older than the verdict.
  - Split: a new file plus an edit of the old one. The old one is `changed`; the new one is `undecided`.
  - Merge: `merged_into` on the absorbed feature. Its id enters the target's digest, so the target is `changed`. If the absorbed feature's verdict is drop, the target goes back to `undecided` until the owner decides on the merged text. `plan sync` closes the absorbed issue with a link.
  - Refine: the status stays `refine`. While the digest is unchanged the orchestrator owes the edit (`refine_pending`); once it is `changed` the owner owes a new verdict.
  - On a built feature the page and `plan decide` offer only take and drop. Drop on a built feature is the status `to-remove`.
- **A dropped need.** A taken feature that needs a feature that is not taken is listed by `plan status` and `plan check --live` (`need_not_taken`). The page shows "Dropping this blocks: …" beside the buttons. `plan next` says why it does not offer the feature.

## 5. Status
One function in `model`. Inputs from the tree: `merged_into`, `built`, the current digest, the ids merged into the feature. Inputs from `GET state`: whether live state is known, the verdict that counts (or none), its digest, whether the label `workshop:building` is set, whether an absorbed feature's verdict is drop. `source`, `unmarked`, `done` and the area's `product` never change the status; they travel with it. First match wins:

| # | When | Status |
|---|---|---|
| 1 | `merged_into` is set | `merged` |
| 2 | `built` is set, live state is known, the verdict is drop | `to-remove` |
| 3 | `built` is set | `built` |
| 4 | live state is not known | `unknown` |
| 5 | the verdict is drop | `dropped` |
| 6 | the verdict is take, its digest differs, an absorbed feature is dropped (owner question 2; the recommendation stands until the owner answers) | `undecided` |
| 7 | the verdict is take and the building label is set | `building` |
| 8 | the verdict is take | `planned` |
| 9 | the verdict is later | `later` |
| 10 | the verdict is refine | `refine` |
| 11 | no verdict counts (none, several labels, a label without its mark) | `undecided` |

- Buildable: the status is `planned` or `building`, every feature in `needs` is built, and no open question blocks it (section 6). `plan next` offers the buildable features whose status is `planned`.
- A test enumerates every combination of the inputs (320) against this table. `unknown` is never shown as undecided: the page says "Decisions cannot be read right now" and hides every count.
- Counts: `built` and `to-remove` count as built; `refine` counts as undecided; `later` has its own count and is in neither "planned" nor "undecided"; `merged` and non-product features count nowhere. "N of M features are planned": N is planned plus building plus built; M is every product feature except `merged` and `dropped`.
- Area state, for product areas, first match wins: `unknown` (live state is not known); `now` (a feature is `building`); `done` (at least one built, none planned); `next` (it holds the planned feature with the lowest position among areas that are not `now`; at most one area); `planned` (at least one planned); `open` (shown as "not decided yet"). In JSON an area state is always under `area.state`, never beside a feature's `status`.
- The open or closed state of a feature's issue is output only. `plan sync`, `POST verdict` and `plan decide` set it: closed as completed when `built`, closed as not planned for `merged_into` or a drop verdict, open otherwise. No reader uses it. For questions and suggestions, closed is the recorded fact.
- This is the rule of the code on `site/workshop-map` and `workshop/building`, with five changes: built comes from the file, not from an issue closed as completed; dropped comes from the verdict, not from an issue closed as not planned; `building` in `GET state` is the raw label and `model` applies "only while taken"; the file verdict is gone; `n` is the position and `STARTER_LAST` is `starter_through`.

## 6. Decisions and questions
- A question is an issue opened by `plan ask --title … --body … [--about feature:<id>…] [--default "<text>"]`. Its body ends with `If nobody answers: <default>` or `No default: this waits for the owner.`, then `<!-- silo-question: {"about":{"features":["<id>"]},"blocks":false} -->`; `about` has the shape it has in a decision. `blocks` is true when there is no default: the features in `about` are not buildable until the question is closed. `plan ask` refuses a title that an open question already has.
- An answer closes the question. The closer is the actor of the newest `closed` event and must be a decider or the bot (`question_closer`). The closing comment is the closer's newest comment made before that event or within 60 seconds after it. The owner answers on the Workshop (`POST decide`: the owner's comment, then the close). An answer given in conversation is recorded with `plan answer <N> --outcome "<text>"`: the bot posts the text ending `<!-- silo-answer: {"source":"recorded","session":"…"} -->` and closes the issue (owner question 1; the recommendation stands until the owner answers).
- Then `plan decision <slug> --answers <N> --text "…"` writes `plan/decisions/<today>-<slug>.json` on a plan branch. It copies `quote` from the closing comment and sets `recorded` when that comment carries `silo-answer`.
- A default: when nobody answered, the agent applies the default and writes a record with `by_default: true`; the question stays open. When the owner later closes it, a new record with the owner's `quote` replaces the first. Two records never answer one question otherwise.
- Proposals that wait for the owner are questions with a default. Decisions are not mirrored to GitHub; the Decisions view (deciders only) is the records plus the open questions.
- Checks (section 11). Offline: `answers_twice`, `quote_missing`, `by_default_invalid`, `ref_unknown`. Live: `question_closer`, `question_no_record`, `record_question_open`, `quote_differs`, `default_overtaken`, `reference_unknown`.

## 7. The mirror
`plan sync [--dry-run]` makes the issues match the plan at `origin/main`. It runs `git fetch origin main` and reads the plan from that ref, never from the working tree, so it gives the same result from any checkout. For each feature it finds the issue (below), creates it when missing, and sets the title to `name` and the body to `what`, a link to the feature on the Workshop (`<site>/workshop#entry-<id>`), the existing verdict mark untouched, then the marker. It sets the open or closed state of section 5, closes the suggestion named by `suggested_in` with a link, creates missing labels, removes a verdict mark that has no label, and removes `workshop:building` from a built feature. It re-reads an issue right before changing its body; a verdict mark lost to a race reads as a label without its mark, never as a verdict. It leaves an issue whose id is not in the ref untouched. It writes nothing into the repository and no verdict label. It waits a second between writes, as `plan carry` does (GitHub allows 80 content-creating requests a minute and 500 an hour; go-live is about 150), stops cleanly on a rate limit, prints what is left, and continues when run again.

- **When.** The workflow `.github/workflows/plan-sync.yml` runs it on every push to `main`, one run at a time, as the bot: the tool makes its own installation token from the repository secrets `WORKSHOP_APP_ID` and `WORKSHOP_PRIVATE_KEY`. When it fails nobody reruns it (agent rule 1): `plan status` reports `mirror_behind` and the orchestrator runs `plan sync` locally. Once the Workshop is live the orchestrator pushes `main` after every green merge (owner question 3; the recommendation stands until the owner answers), so local `main` and the mirror differ for minutes.
- **Another ref.** `--ref <ref>` is accepted by `plan sync`, `plan carry` and `plan check --live` only while `origin/main` has no `plan/order.json`, that is during go-live.
- **Issue on demand.** `plan decide`, `plan start` and `plan ask --about` on a feature without an issue create it from the definition they read. The next sync corrects it. The page ignores an issue whose id is not in its bundle.
- **The issue of a feature** is the lowest-numbered issue that the bot created and that carries the feature's marker. An issue with the marker that the bot did not create is ignored (`entry_foreign`). `plan sync` and the record commands list the newest feature issues again right before they create one. Two can still appear (a record command meeting the workflow). `plan sync` closes the higher one with links both ways when it carries no verdict label. When it carries one, sync leaves it open and `issue_duplicate` names both: the orchestrator runs `plan decide` again, which writes on the lowest and clears the other.
- **Reading live state.** The tool reads `GET <site>/api/workshop/state`: one call, no token, no GitHub quota. Every command prints `fetchedAt`, the age it computes from it, and `stale`. The age is normally under 40 seconds (the server refreshes after 30, the CDN keeps an answer 10). While GitHub refuses the server an answer can be 10 minutes old before the server marks it `stale`, so the tool warns when the age is over 60 seconds. `plan show <id>`, `plan decide` and `plan start` read their one issue from GitHub and are never older than the call. When the site does not answer, the tool reads the three label lists from GitHub and applies the same `model` function.
- **Reading after a write.** A tool write does not pass through the server, so the site shows it later. Each write command reads the issue back from GitHub, prints the new record, and keeps it in `~/.local/state/silo-plan/recent-writes.json`. A read command uses a kept record in place of the site's until the site's `fetchedAt` is 15 seconds later than that write; a record is deleted after 10 minutes. Nothing else reads this file.
- **Previews.** A deployment whose `VERCEL_ENV` is `preview` answers `GET state` and `GET issue` by reading production's public answers and refuses every write, whatever secrets it has. Production never does either.

**`GET state`.** `model` holds the function from a GitHub issue to its record, with the bot's identity as an input, and a fixture of the full answer that the server, the tool and the fake site are tested against. Every record keeps today's keys (`kind`, `entry`, `title`, `state`, `reason`, `verdict`, `up`, `comments`, `author`, `bot`, `createdAt`, `closedAt`). The new keys, shown here without the old ones, are absent when they do not apply:
```json
{ "repo": "amontlabs/silo", "fetchedAt": "2026-11-20T10:02:11Z",
  "issues": {
    "57": { "kind": "entry", "entry": "fork", "verdict": "take", "digest": "1:573a8a06ef00fb26", "source": "click", "commit": "<40 hex characters>" },
    "56": { "kind": "entry", "entry": "checkpoints", "verdict": "take", "digest": "1:…", "source": "recorded", "session": "…", "building": true },
    "70": { "kind": "entry", "entry": "undo", "verdict": null, "unmarked": "take" },
    "71": { "kind": "entry", "entry": "diff", "verdict": null, "unlabelled": true },
    "72": { "kind": "entry", "entry": null, "verdict": null, "foreign": true },
    "41": { "kind": "decision", "about": { "features": ["fork"] }, "blocks": false },
    "64": { "kind": "suggestion", "seen": true } } }
```
`"stale": true` is added only when the answer is over 10 minutes old. `building` is the raw label. `session` and `decision` come from the mark when it has them. `unmarked` is the label's value, or `several`.

**Permanent names.** Once the first issue exists these never change, and a label is never renamed or deleted: receipts point at its events. A new meaning gets a new name beside the old.

| Kind | Name | Today |
|---|---|---|
| Label | `workshop:entry` (0e8a16, "Workshop: an entry of the feature list"), `workshop:decision` (1d76db, "Workshop: a question for the owner"), `workshop:suggestion` (fbca04, "Workshop: a feature suggested by a visitor") | `tools/workshop/src/names.js` (name, colour, description), `website/server/issues.mjs` (name) |
| Label | `verdict:take` (2da44e), `verdict:refine` (bf8700), `verdict:later` (8c959f), `verdict:drop` (cf222e), each "The owner's verdict: <value>" | same two files |
| Label | `workshop:building` (8250df, "Being built now") | same two files, on `workshop/building` only |
| Label | `workshop:seen` (d4c5f9, "Workshop: a suggestion the orchestrator has read") | new |
| Body marker | `<!-- silo-entry: <id> -->`, last line of the body | same two files; the id pattern narrows from `[A-Za-z0-9._-]+` (tool) and `[A-Za-z0-9_.-]{1,100}` (server) to the feature id pattern |
| Body marks | `<!-- silo-verdict: <json> -->`, `<!-- silo-question: <json> -->` | new |
| Comment marks | `<!-- silo-note -->`, `<!-- silo-build: <json> -->`, `<!-- silo-answer: <json> -->`, last line of the comment | new |
| Values | verdicts `take`, `refine`, `later`, `drop`; sources `click`, `recorded`, `carried`; digest prefix `1:` | verdicts in the same two files; the rest new |
| Other | repository `amontlabs/silo`, site `https://silo.amontlabs.com`, Keychain service `silo-workshop`, credentials `WORKSHOP_APP_ID`, `WORKSHOP_CLIENT_ID`, `WORKSHOP_CLIENT_SECRET`, `WORKSHOP_PRIVATE_KEY`, cuts of 20,000 (body) and 5,000 (comment) characters | `tools/workshop/src/names.js`; the cuts in `website/server/issues.mjs` |

## 8. The tool
`tools/plan/bin/plan`, Node 26, no dependency. "Who" is the orchestrator, a worker, or both. "Reads": T the working tree of the checkout it runs in, H that checkout's `HEAD`, M the ref `main`, O `origin/main`, S the site's `GET state`, G GitHub directly. `session` is `--session <name>`, by default `<host>:<branch>@<commit>`. Only `plan start` commits; every other command that writes files prints the `git commit` line to run. Exit codes: 0 done, 1 refused or errors found, 2 wrong use, 3 live state could not be read. `plan status` exits 0 whenever it could read; `plan check --live` exits 1 on an error.

| Command | Who | Reads | Writes | Output |
|---|---|---|---|---|
| `plan` | both | T, S | nothing | The guide: the rule in five lines, the layout, every key with its description, the rules that no check enforces (section 11), every command with who may run it, the state in ten lines, the next command to run: the orchestrator's on `main` and `plan/*`, the worker's on any other branch |
| `plan show <id>` | both | T, S, G (one issue) | nothing | One feature with everything (sample below) |
| `plan list`, `plan next` | both | T, S | nothing | `list`: every feature in order with position, id, name, area, status. `next`: the features offered, ordered by `needs`, then product before work, then position; then every other taken feature with the reasons it is not offered |
| `plan status [--since <time>]` | orchestrator | M, O, S, G, git branches | nothing | Obligations only, each with the command that clears it (the live checks of section 11); then what waits for the owner. `--since` adds comments newer than the time |
| `plan check --live` | orchestrator | M, O, S, G, git branches | nothing | The same checks on the same refs, so the same result as `plan status` from any checkout |
| `plan legacy <id>` | a worker started for that lookup | T | nothing | The legacy record |
| `plan check [--base <ref>]`, `plan format` | both | T | `format`: canonical form of files under `plan/` | Errors; with `--base` also `lane: plan` or `lane: code`; the files `format` changed |
| `plan new <id> --name … --what … --area <id> [--from <N>]`, `plan move <id>`, each with `--after <id>` or `--first-in <stage>` or `--first-in work` | orchestrator, plan branch | T | the feature file, `plan/order.json`; `--from` sets `suggested_in` and puts `workshop:seen` on the suggestion | The file, the position |
| `plan decision <slug> --text … [--who …] [--answers <N>] [--by-default] [--about feature:<id> or area:<id> or legacy:<id> …] [--replaces <id>…] [--doc <path>]` | orchestrator, plan branch | T, G | the decision file | The file |
| `plan decide <id> <verdict> …` | orchestrator, on `main` or `plan/*` | H, G | labels, issue state, verdict mark, note comment | Status before and after; what the verdict blocks. Refuses on another branch (`wrong_checkout`), and when the feature's file differs from `HEAD` (`uncommitted`) |
| `plan start <id>` | orchestrator, on a build branch (not `main`, not `plan/*`) | H, G | the receipt and its commit, label, build comment | The receipt; the branch |
| `plan stop <id>`, `plan answer <N> --outcome …`, `plan triage <N> --keep` or `--decline "<note>"` | orchestrator, any checkout | G | `stop`: removes the label. `answer`: closing comment, close. `triage`: `workshop:seen`, or a comment and closed as not planned | The record read back |
| `plan ask …`, `plan ask --file <json>` | orchestrator, any checkout | T, G | one question, or every question of the file that is not open yet | Number, link |
| `plan sync`, `plan carry` | orchestrator; the workflow | O, G | issues. `carry` sets a verdict only where none counts, so a second run changes nothing | Created, updated, closed, left; "in sync" |
| `plan setup`, `plan store`, `plan doctor` | owner, orchestrator | G | Keychain, `vercel.env` | As `tools/workshop/` today |

Every command takes `--json` and then prints one document `{ "ok", "ref", "live", "data" }`. `ref` (branch, commit, uncommitted changes, distance from `origin/main`) and `live` (source, `fetchedAt`, age, `stale`) are always present, in the text output too. A live value that could not be read is the string `"unknown"`, never an absent key. Errors are `{ "code", "message", "file", "field", "fix" }`; a live failure adds `"last_known": { "fetchedAt", "stale" }` or `null`. `data` is: for `plan show` the object below; for `plan list` `{ "features": [ … ] }` with the keys `id`, `name`, `area`, `position`, `status`; for `plan next` `{ "offered": [ … ], "waiting": [ { "id", "reasons" } ] }`; for `plan status` `{ "building": [ … ], "obligations": [ { "code", "about", "message", "fix" } ], "waiting_for_owner": [ … ] }`; for `plan check` `{ "errors": [ … ], "lane" }`. Both samples come from one fixture, in which `fork` is taken and not started.
```json
{ "ok": true,
  "ref": { "branch": "main", "commit": "4799af1e6", "uncommitted": false, "ahead_of_origin_main": 22, "behind_origin_main": 0 },
  "live": { "from": "site", "fetchedAt": "2026-11-20T10:02:11Z", "age_seconds": 12, "stale": false },
  "data": { "id": "fork", "name": "Fork a computer", "what": "Copy a computer, …", "status": "planned", "changed": false,
    "area": { "id": "forks", "title": "Forks", "state": "planned" }, "stage": { "id": "loop", "title": "The loop", "position": 16, "starter": true },
    "rules": [ { "id": "cold-boot", "text": "…" } ], "surfaces": { "app": "button:Fork", "cli": "silo fork · drop" },
    "needs": [ { "id": "checkpoints", "status": "building" } ], "needed_by": [ { "id": "quiet-forks", "status": "undecided" } ],
    "verdict": { "value": "take", "by": "<login>", "at": "2026-11-18T09:14:03Z", "source": "click", "note": null, "digest": "1:573a8a06ef00fb26", "text": "current" },
    "issue": { "number": 57, "url": "https://github.com/amontlabs/silo/issues/57", "votes": 3, "comments": 2 },
    "buildable": { "value": false, "reasons": ["needs checkpoints: building, not built"] },
    "receipt": null, "done": null, "built": null, "tests": [], "recordings": [],
    "questions": [ { "number": 41, "title": "…", "open": true, "blocks": false } ], "decisions": [], "obligations": [],
    "prose": { "behaviour": "…", "notes": null },
    "legacy": { "lessons": ["A fork always cold-boots the disks, never memory, …"], "old_code": { "run": "plan legacy fork", "who": "a worker started for that lookup" } },
    "to_change": [ { "what": "name, what, rules, needs, surfaces, area", "how": "edit plan/features/fork.json on a branch plan/<topic>", "who": "orchestrator" },
                   { "what": "order", "run": "plan move fork --after <id>", "who": "orchestrator" }, { "what": "verdict", "run": "plan decide fork <verdict>", "who": "orchestrator" },
                   { "what": "start of the build", "run": "plan start fork", "who": "orchestrator" },
                   { "what": "built, done, tests, recordings", "how": "edit plan/features/fork.json in the code's merge", "who": "worker" } ] } }
```
`verdict.text` is `current`, `changed` or `checkout_behind` (section 4).
```
plan status    read main@4799af1e6 and origin/main@1c0ffee42 (main is 22 ahead)    live: site, 2026-11-20T10:02:11Z, 12 s old
being built    checkpoints (build/checkpoints)
next           silo-cli is offered. fork waits: needs checkpoints (building)
obligations
  plan_branch_unmerged   plan/reword-merge, last commit 3f2a91c, 2 hours old     section 10, steps 4 to 6
  question_no_record     #44 is closed by a decider, no record                   plan decision <slug> --answers 44 --text "…"
  suggestion_untriaged   #63 "Snapshots to S3", 4 votes                          plan triage 63 --keep | --decline "…" | plan new <id> --from 63
waits for the owner (nothing to run)    recorded: terminal, take, 2026-11-20    changed: merge, text changed since the verdict
```

## 9. The modules
- `tools/plan/model/`: names, patterns, marker and mark readers and writers, the digest (with its own SHA-256, so it is synchronous), the function from a GitHub issue to its `GET state` record, the calls of a verdict write in order, the status table, area states, counts, positions, buildable reasons, the wanted title, body and state of an issue. It imports nothing outside its folder and no `node:` module; a test enforces it. Plain ESM with JSDoc types.
- `tools/plan/read/`: reads `plan/` from a directory or from a git ref into one plan object. Imports `model` and `node:`.
- `tools/plan/check/`: the schema checker, the rules across files, `--base`, the live checks. Imports `model`, `read`, `node:`.
- The Workshop page (`website/src/workshop/`) imports `model` only, by a relative path; TypeScript reads it with `allowJs`.
- The build plugins (`workshopData`, `homePlan` in `website/vite.config.ts`) import `read`, `check` and `model`. The build stops on any `plan check` error and embeds its commit. The home page gets the first stage's `line` at build time; its count is filled at view time from `GET state` through `model` and is absent while live state is unknown.
- The server function (`website/server/`) imports `model` only. `website/scripts/prepare-vercel.mjs` copies `tools/plan/model/*.js` into the function and keeps the repository's relative layout (`website/server/`, `website/api/`, `tools/plan/model/`), so one import path works in both. It refuses any other file outside `website/`.
- The tool, the gate and CI use all three through `tools/plan/bin/plan`. No generated file is committed.

## 10. How a plan change travels
A plan change made in conversation, as the orchestrator runs it:
1. `git worktree add <path> -b plan/<topic> main`. Not `atelier start`: a plan branch is not an item. Never work in the main checkout.
2. Edit the files, or run `plan new`, `plan move`, `plan decision`. Then `plan format`, and `git commit -m "Plan: …" -- plan/`. The fact is in git the moment it is decided.
3. If the owner also gave a verdict: `plan decide <id> <verdict>`, from this worktree, after the commit.
4. If `main` moved: `git merge main` in this worktree. Never rebase: a verdict mark names a commit, and a rebase would leave it on no branch.
5. `plan check --base main` passes and prints `lane: plan`. It does when the diff touches only files under `plan/`, adds or removes no area, changes no area's `product`, and does not touch `plan/schema/`. Such a branch needs no other proof: the site's build-time read is the same code, and no test reads the real `plan/` (tests use fixtures). `lane: code` goes through `atelier merge`.
6. In the main checkout, when `atelier status` shows no merge running and no promotion pending: `git merge --ff-only plan/<topic>`. This is the one exception to orchestrator rule 4b ("anything else goes through `atelier merge`"). Then `git worktree remove <path>` and `git branch -d plan/<topic>`.
7. Once the Workshop is live: push `main`.

- `plan status` lists every unmerged `plan/*` branch with its last commit and its age. The session that made a branch merges it; another session merges it only when its last commit is older than an hour.
- **Workers.** Under `plan/` a worker changes only `built`, `done`, `tests` and `recordings` of the feature its task names, so `built` and the receipt merge with the code. This is a rule, not a check.
- **The gate and CI.** `app/SiloUI/scripts/ci/scripts.sh` gains the steps `plan-tests` (`npm --prefix tools/plan test`, from build item 1) and `plan-check` (`tools/plan/bin/plan check --base <base>`, at the cutover; the gate's base is `main`, CI's is the pull request's base or the commit before the push). `app/SiloUI/scripts/ci-native-changes.py` lists `plan/` and `tools/plan/`, so CI skips the Rust jobs. The restarted tree carries `plan/`, `tools/plan/` and the step `plan check --base main` in `.atelier/atelier.toml`. When `main` starts again as that tree, the last commit of the old `main` is tagged first, so the commits that verdict marks name stay readable; otherwise "before" is lost, and nothing else.

The lines for the rule files. `AGENTS.md` is generated from `.atelier/project/binding.md` by `atelier rules`; never edit `AGENTS.md`. Two lines rest on open points: the orchestrator's push line (owner question 3; the recommendation stands until the owner answers) and the worker's `built` line (owner question 5; the recommendation stands until the owner answers).
```
# .atelier/project/binding.md, replacing its "Product" and "restarted" lines
- The plan is in `plan/`. For anything about the plan, run `tools/plan/bin/plan`: it prints the guide and the command to run next. Plan facts live nowhere else.
- Nothing is built without a feature the owner took. `plan next` lists what may be built.
- Silo is being restarted from an empty tree. The old code is at tag `legacy-2026-11-16`; it is read only by an agent started for that lookup, from `plan legacy <id>`. Add nothing to the current tree except `plan/`, `tools/plan/`, `website/`, `docs/` and the gate and CI wiring for them.
# .atelier/project/orchestrator.md, replacing its dashboard, journey and recording lines
- After `atelier status`, run `tools/plan/bin/plan status`. Every line is an obligation with the command that clears it. The owner's page is the Workshop.
- What the owner says in conversation is recorded at once: a verdict with `plan decide`, an answer with `plan answer` then `plan decision`, any other plan change on a branch `plan/<topic>`. When a change and a verdict come together, commit the change first and run `plan decide` from that branch.
- A plan branch: `git worktree add <path> -b plan/<topic> main`; edit, `plan format`, commit by path; `git merge main` there if `main` moved, never a rebase; `plan check --base main` must pass and print `lane: plan`; then in the main checkout, only when `atelier status` shows no merge running and no promotion pending, `git merge --ff-only plan/<topic>`. This is the one exception to rule 4b. `lane: code` goes through `atelier merge`. Merge a plan branch you did not make only when its last commit is older than an hour.
- Every `plan` command that writes is yours alone, except `plan format`. Never put one in a worker's task.
- To build a feature, in this order: `plan next` offers it; `atelier start <branch>`; `plan start <id>` in that worktree; `atelier task` with the feature id in the goal. After a merge that leaves the feature unfinished: `plan stop <id>`.
- A question for the owner is `plan ask` with a default. Never stop the build to wait for an answer.
- Once the Workshop is live, push `main` after every green merge. If `plan status` says `mirror_behind`, run `plan sync`.
# .atelier/project/worker.md, three new lines
- Your task names a feature id. After `atelier check --compile-only`, run `tools/plan/bin/plan show <id>`: it prints the definition, the rules, what the feature needs and what you may edit. If it shows no receipt, stop and report: the orchestrator has not started the feature.
- Under `plan/` you change only `built`, `done`, `tests` and `recordings` in `plan/features/<id>.json`, in the same commit as the code. Set `built` to today's date (UTC) only when every rule holds and every surface in `surfaces` exists; otherwise set `done` to the surfaces that exist, `[]` when none does. A test that proves a rule has `<feature id>/<rule id>` in its name.
- Never run a `plan` command that writes, except `plan format`. Never rename or delete a file under `plan/`. Run `plan check --base main` before your report.
```

## 11. Checks
Offline, `plan check`: deterministic; it runs in the gate, in CI and in the site build. Rows marked (base) need `--base <ref>` and look at the diff from the ref.

| Code | Meaning | Fix it prints |
|---|---|---|
| `json_invalid`, `not_canonical`, `schema`, `schema_keyword` | A file is not JSON, or not in canonical form; a key is missing, unknown, or of the wrong type or shape; a schema uses a keyword outside the subset | The line and `plan format`; the file, the key, the allowed keys or values |
| `file_name`, `unknown_file` | A file name does not match its id pattern; a file under `plan/` that section 3 does not name (also an `.md` without its `.json`) | The pattern; the folders that exist |
| `ref_unknown` | `area`, `needs`, `merged_into`, `about`, `replaces`, `starter_through`, an id in `plan/order.json` or a key of `plan/carried-verdicts.json` names nothing | The ids that exist |
| `order_missing`, `order_twice`, `order_wrong_list` | A feature is not listed, is listed twice, or is in a stage while its area has `"product": false` (or under `work` while it has not) | `plan move <id> --after <id>` |
| `needs_cycle`, `need_later`, `need_merged` | A cycle; a need that comes later in the same list; a need that was absorbed | Both repairs: move the feature down, or move the need up; for an absorbed need, its target |
| `merged_chain`, `name_twice`, `rule_id_twice`, `area_empty` | The target of `merged_into` is itself absorbed; two features share a name; two rules share an id; an area has no feature | The two files |
| `prose_heading` | A heading other than `## Behaviour` and `## Notes`, the wrong order, or text before the first heading | The allowed headings |
| `built_without_receipt`, `receipt_ref` | `built` without `receipt`; `receipt.ref` is not an event URL of this repository | Ask the orchestrator to run `plan start <id>` |
| `built_without_proof`, `built_before_need`, `done_invalid` | `built` with no entry in `tests` or `recordings`; with an unbuilt need; `done` beside `built`, or naming a key that is not in `surfaces` | Name a test or a recording; build the need first; remove `done` |
| `file_missing`, `recording_size` | A named test, recording or `doc` does not exist; a recording is over 10 MB | The path |
| `answers_twice`, `quote_missing`, `by_default_invalid` | Two records answer one question and the newer does not replace a `by_default` one; `answers` or `recorded` without `quote`; `by_default` without `answers`, or with `quote` | `--replaces <id>`; `plan decision … --answers <N>` |
| `legacy_pair` | `"carried": true` without a feature of that id; `false` with one, or without `name`, `what`, `why` | The file |
| `product_lint` | `plan/PRODUCT.md` holds a number followed by "features", "entries" or "stages", or a stage or area title in a list item. A lint: it proves less than its name | The line |
| `old_file` | A file exists under `docs/product/` or `tools/workshop/` | Delete it; section 2 names the fact's home |
| `receipt_digest` (base) | The diff adds `built` or changes `done`, and `receipt.digest` differs from the current digest: the text changed since the build started | Read the new text in `plan show <id>`; the orchestrator runs `plan start <id>` again |
| `started_unfinished` (base) | The tree to merge holds `receipt` with neither `built` nor `done`: a build that forgot to say what it finished | Set `built` with `tests` or `recordings`, or set `done` (`[]` when nothing is usable yet) |
| `file_removed`, `area_removed`, `rule_renamed` (base) | A file under `plan/features/`, `plan/decisions/` or `plan/legacy/` is gone or renamed; an area id is gone; a rule's text moved to another id | Restore it; retire a feature with `merged_into` or a drop verdict, a decision with `replaces` |

Rules that no check enforces; the guide prints them: a rule id is never reused; every surface exists before `built`; a decision's `text` holds no link; only the orchestrator changes a definition or `plan/order.json`; only `plan start` writes `receipt`; a verdict is never also a decision record; no label is renamed or deleted; no test reads the real `plan/`.

Live, `plan check --live` and `plan status`. Both read the refs `main` and `origin/main`, never the working tree. Each is an error unless marked (info). (G) after a code means that code needs GitHub directly; the others read `GET state` and git.

| Code | Meaning | Fix it prints |
|---|---|---|
| `mirror_behind` (G) | A feature of `origin/main` has no issue, or its title, body or state differs; a label is missing; a built feature still carries `workshop:building` | `plan sync` |
| `local_ahead` (info) | `main` is ahead of `origin/main` in `plan/` | Nothing until the push |
| `issue_orphan`, `entry_foreign` | A bot issue whose id is no feature of `origin/main`, `main` or a `plan/*` branch; an issue with a marker that the bot did not create | Merge the plan branch or close the issue; remove its `workshop:entry` label |
| `issue_duplicate` | Two open bot issues carry one marker and the higher one has a verdict label | `plan decide <id> <verdict>` again, then `plan sync` |
| `verdict_several`, `verdict_unmarked`, `verdict_unlabelled` | Several verdict labels; a label with no matching verdict mark; a verdict mark with no label | The owner decides on the Workshop, or `plan decide`; for a mark alone, `plan decide <id> clear` |
| `verdict_actor` (G) | The newest `labeled` event of the verdict label is by neither a decider nor the bot; the line names the actor, its role and its App | The owner confirms or clears |
| `building_not_taken`, `building_stale` (G) | The building label on a feature that is not taken; the branch of its build comment is missing, or merged into `main` while `built` is not set | `plan stop <id>`, the normal step after a merge that leaves a feature unfinished; or `plan start <id>` on the branch that continues |
| `receipt_invalid` (G), `built_not_taken` | `receipt.ref` is not a `labeled` event of `verdict:take` on this feature's issue by a decider or the bot; a built feature whose verdict is not take | `plan start <id>` again; the owner takes it again, or its code is removed (`to-remove`) |
| `need_not_taken`, `absorbed_dropped` | A taken feature needs one that is not taken; row 6 of section 5 holds for a feature | The features blocked; the owner decides |
| `refine_pending` | The verdict is refine and the text has not changed since | Edit the feature on a plan branch; the note is in `plan show <id>` |
| `question_closer` (G), `question_no_record` (G) | A question closed by someone who is neither a decider nor the bot; a question closed by a decider or the bot, without a record | Reopen it on GitHub, it is not an answer; `plan decision … --answers <N>` |
| `record_question_open`, `default_overtaken` (G) | A record, not `by_default`, whose question is open; the owner closed a question whose only record is `by_default` | `plan answer <N> --outcome …`; a new record with `--replaces` |
| `quote_differs` (G) | `quote` is not the closing comment's text, or `recorded` does not say whether that comment carries `silo-answer` | Copy the text; set `recorded` |
| `reference_unknown` | `answers` is not a question, `suggested_in` is not a suggestion, or a verdict mark's `decision` names no record | The issue or record to name |
| `suggestion_untriaged` | An open suggestion without `workshop:seen` | `plan triage <N> …`, or `plan new <id> --from <N>` |
| `carried_pending` | `plan/carried-verdicts.json` exists while any feature issue exists | `plan carry`, then delete the file on a plan branch |
| `plan_branch_unmerged` | A local `plan/*` branch is not merged into `main`; the line gives its last commit and its age | Section 10, steps 4 to 6, when it is yours or older than an hour |
| `recorded_pending`, `changed_pending` (info) | Recorded or carried verdicts and answers the owner has not confirmed; features changed since their verdict | Nothing: they wait for the owner |

## 12. Migration and go-live
**Order.**
1. Merge the three open branches first: `workshop/building`, then `site/workshop-map`, then `site/home` (it must follow the new `parseFeatures` signature). The migration rewrites the files they touch.
2. Build items 0 to 8 of section 14. Until the cutover the old files stay the only home: the build plugins get the plan object from the converter (`tools/plan/migrate/from-old.js`), in memory. Nothing under `plan/` except `plan/schema/` is on `main`. Until go-live there is no live state: the page shows `unknown`.
3. The cutover, one merge that merges alone (item 9).
4. Go-live, run once by the orchestrator with the owner, before the first push.
5. The removals after go-live (item 10).

**Before the old files are deleted** (the proof of the cutover):
- [ ] Every (id, key) of the 119 lines of `docs/product/features.jsonl` maps to exactly one place: 110 feature files, 119 legacy records, 9 of them with `"carried": false`. The work features come on top of the 110.
- [ ] `plan/order.json`, flattened, equals the order by `n` and by line; 12 stages (`gone` is no stage); `starter_through` is `living`; 16 areas plus `work`.
- [ ] 26 decision records whose `text` equals the old lines without their links, plus one that names the three dropped leftovers (`hosted-reviewer`, `project-folder`, `resident-agents`) in `about.legacy`. `replaces` is set by hand where the old lines contradict each other on where the owner decides.
- [ ] `plan/carried-verdicts.json` holds the 13 takes of the old file.
- [ ] `plan/PRODUCT.md` no longer lists the stages; `product_lint` passes.
- [ ] The dashboard store has no unacknowledged verdict and no open item.
- [ ] The twelve proposals (eight in `docs/product/RESTART.md`, four in agent memory) are drafted as questions in `tools/plan/migrate/questions.json`; the work still ahead in "Where the work stands" is features under `work`.
- [ ] The rules of `SHARED-3.md` and `DECISIONS-3.md` are tests of `model`.
- [ ] `grep` finds `features.jsonl`, `stages.json`, `DECISIONS.md`, `old-code-map`, `docs/product/` and `tools/workshop/` nowhere in code, tests, CI scripts (`app/SiloUI/scripts/ci/scripts.sh`, `app/SiloUI/scripts/ci-native-changes.py`, `website/scripts/fake-github.mjs`, `website/scripts/workshop-dev.mjs`), rules or READMEs, except in `tools/plan/check/` (the `old_file` check) and `docs/PLAN.md`.
- [ ] `plan check --base main` runs in the gate and in CI and fails on `old_file`.
- [ ] `plan doctor` passes: the owner has created and installed the GitHub App and set the Vercel variables and the two workflow secrets. These steps precede the first issue, so they are lines here, not questions.

**Go-live.** Nothing pushes `main` between steps 1 and 6. (1) `plan sync --ref main`, repeated until it prints "in sync": the labels and every issue. (2) `plan carry --ref main`: each carried verdict as `source: carried` with the commit of the ref. (3) `plan ask --file tools/plan/migrate/questions.json`. (4) Delete `plan/carried-verdicts.json` through the plan lane. (5) `plan check --live --ref main` reports no error. (6) Push `main`: the site deploys, and the workflow's first run finds nothing to change and writes as the bot; that run is the proof that repository secrets reach a push run, and if it fails `plan status` says `mirror_behind`. (7) The owner opens the Workshop: "Carried over from the old list" shows the 13 with one Confirm all (owner question 1; the recommendation stands until the owner answers). (8) Check on the live site what `docs/WORKSHOP.md` lists under "Not verified until deployed". Between the cutover and step 2, a verdict given in conversation is added to `plan/carried-verdicts.json`, with its note.

**After go-live, removed.** In the repository: `tools/dashboard/` and its CI steps; `docs/OWNER-DASHBOARD.md`, `docs/DIRECTION.md`, `docs/ROADMAP.md` and the links to them in `docs/README.md` and `README.md`; `tools/plan/migrate/`. Outside it, by the orchestrator's hand: the dashboard store; `sweep/out/` and the write mode of the sweep script; the agent memory files `restart-from-scratch.md`, `product-pipeline-setup.md`, `product-direction-agent-inside.md`, `plan-single-source-of-truth.md` with their index lines; the notes `SHARED-3.md` and `DECISIONS-3.md` in `~/.local/state/silo-dashboard/design/workshop-build/`.

| What could be lost | Guard |
|---|---|
| The 16 verdicts; who and when, for the 11 that say it only inside `why` | 13 in `plan/carried-verdicts.json`, 3 in one decision record; the file is deleted only after `plan carry`. `suggest.why` keeps the sentence verbatim |
| The order; the nine leftovers | The comparison against `n` and line order; legacy records, and the six without a verdict are not recorded as dropped |
| The twelve proposals | Drafted questions are a precondition; asked in go-live step 3 |
| The status and area rules kept outside the repository | Tests of `model`; `docs/PLAN.md` |
| Relative links inside decision text | Removed by the converter; a pointer is the `doc` key, which is checked |
| "None" against "not said" for `app: null` and `old_lines: 0` | Stated once in section 3 |
| The owner's place to decide between cutover and go-live; an old file brought back by a worker's rebase | The cutover waits for `plan doctor` and go-live follows at once; `old_file` |

## 13. Deferred, on purpose
| Not built now | Decided now |
|---|---|
| Rules to tests: a tracer that lists unproven rules | Rules are data with permanent ids; a test names a rule by having `<feature id>/<rule id>` in its name. When the tracer exists, `tests` and `built` are derived and the keys go in one commit |
| The public changelog | `built` is a date and `name` is the public title; the changelog is generated from them. The restarted tree has no changeset files |
| Recordings policy: who records, which features must have one | Recordings are files named by `recordings`, at most 10 MB, checked. No rule requires one |
| `surfaces` against the engine's own model | `surfaces` has the keys `app` and `cli` only. When `calls-and-events` defines every call and command, `surfaces` is checked against it or derived from it |
| Native "blocked by" on GitHub | `needs` is the only home. If mirrored, only `plan sync` writes it and nothing reads it back |
| `plan comment`, filters on `plan list` | Added when a need appears; no format depends on them |

## 14. Build items
In order; each depends only on earlier ones and is one branch through the gate. Items 3 to 8 start after step 1 of section 12. Items 2 and 3 can run side by side, then 4 and 5. A line "default:" is the builder's to change, with the reason in its report.

| # | Item | Delivers | Files | Depends on | Proven by |
|---|---|---|---|---|---|
| 0 | allowed paths (merges alone) | The "Add nothing" line of `.atelier/project/binding.md` also allows `plan/`, `tools/plan/`, `website/`, `docs/`, `.atelier/`, `.github/workflows/plan-sync.yml` and `app/SiloUI/scripts/`. No other rule changes: `docs/product/RESTART.md` stays the first read until the cutover | `.atelier/project/binding.md`, `AGENTS.md` through `atelier rules` | nothing | `atelier rules` leaves no diff |
| 1 | model | Section 9's `model`, the `GET state` fixture, the gate step `plan-tests`, the two paths in `app/SiloUI/scripts/ci-native-changes.py`. default: the bot is recognised by the issue's `performed_via_github_app` id equal to `WORKSHOP_APP_ID` | `tools/plan/model/`, `tools/plan/test/`, `tools/plan/package.json`, `app/SiloUI/scripts/` | 0 | The 320-row status test; the area rules of `SHARED-3.md` as tests; digest vectors against `node:crypto`, the first being `1:573a8a06ef00fb26`; the no-import test |
| 2 | read and check | `read`, `check`, the schema files, `plan check`, `plan format` | `tools/plan/read/`, `tools/plan/check/`, `tools/plan/cli/`, `tools/plan/bin/plan`, `plan/schema/` | 1 | One failing fixture per offline code; `--base` on a scratch repository |
| 3 | server | Names, the record function and the verdict write from `model`; the new `GET state` keys; `POST verdict` with `digest` and `commit`, the mark and the marked note; the `VERCEL_ENV` rule; the function packaged with `model`. default: the one fake GitHub is `website/scripts/fake-github.mjs`, and it gains marks, events and `performed_via_github_app`. default: the package copies `tools/plan/package.json` so the model files load as modules. default: a decider is read from `role_name` only | `website/server/`, `website/api/`, `website/scripts/` | 1 | Server tests against the `GET state` fixture; the packaged function loads alone. Once, by the orchestrator on a test repository, written into `docs/WORKSHOP.md`: what the permission endpoint returns in `role_name` and `permission` for a Moderate collaborator |
| 4 | converter | Old files to a plan object and to a `plan/` tree; drafts of `needs`, `replaces`, the questions, the work features | `tools/plan/migrate/` | 2 | The checklist of section 12 as a test on the real files; `plan check` passes on the tree it writes |
| 5 | tool, reading | `plan`, `show`, `list`, `next`, `legacy`; the live reader (site first, then GitHub through the record function). default: the goldens of `show` and `next` come from one fixture | `tools/plan/cli/`, `tools/plan/live/` | 2 | Golden outputs; a fake site that answers, is old, is stale, fails |
| 6 | tool, writing | `status` and the live checks; `sync` from a ref, `decide`, `start`, `stop`, `ask`, `answer`, `triage`, `carry`, `new`, `move`, `decision`; the recent writes; `setup`, `store`, `doctor` moved over. default: its tests import `website/scripts/fake-github.mjs`. default: a receipt's event is read once in a run and kept by id | `tools/plan/cli/`, `tools/plan/github/`, `tools/plan/live/` | 3, 5 | Against the fake GitHub: sync twice changes nothing, resumes, gives one result from two checkouts; a duplicate with and without a verdict label; issue on demand; the write order, cut after each call; the receipt commit; every refusal; one case per live code. Once, on a test repository: what `label.name` of a `labeled` event shows after the label is renamed |
| 7 | page | The plan object and `model` everywhere; `unknown`; "Changed since you took it" with its three cases, "Recorded for you", "Carried over from the old list"; "Dropping this blocks"; work listed apart; a suggestion offers "Make it a feature", not a verdict; an address resolves by feature id alone. default: Confirm all sends one `POST verdict` at a time and waits when the server asks. default: `server.fs.allow` holds the repository root for the dev server | `website/src/workshop/`, `workshopData` in `website/vite.config.ts` | 1, 3, 4 | Page tests on fixtures; no status rule left outside `model` |
| 8 | home page | The first stage's line at build time; the count at view time | `homePlan` in `website/vite.config.ts`, `website/src/home-plan.ts` | 7 | Home tests: known, unknown, stale |
| 9 | cutover (merges alone) | `plan/` written by the converter; `docs/product/` and `tools/workshop/` deleted; plugins switched to `read`; the gate and CI step `plan-check`; the workflow; the three rule files and `atelier rules`; `docs/PLAN.md` (this design, the tools examined and their gaps), `docs/WORKSHOP.md`, `docs/README.md` | `plan/`, `.atelier/`, `.github/workflows/plan-sync.yml`, `app/SiloUI/scripts/`, `docs/`, `website/vite.config.ts` | 0 to 8 | The checklist of section 12; the site builds; `plan check --base main` passes. The workflow writing as the bot is proven in go-live step 6 |
| 10 | after go-live | The removals of section 12 that are in the repository | as listed there | 9, and go-live | `grep`; the gate. What is outside the repository the orchestrator removes by hand and lists in its report |

## Decided while writing
- The verdict's digest, source and commit are a mark in the issue body, not a comment: the server holds only the newest 1,500 comments of the repository, so a comment would fall out of what the page can read.
- One routine writes a verdict, mark last when set and mark first when cleared; a mark without a label is no verdict and `plan sync` removes it.
- Label and mark must agree on an issue the bot created. That keeps out Triage and GitHub's label menu; it does not keep out Moderate, another App or a workflow, which `verdict_actor` reports.
- One function in `model` turns a GitHub issue into its `GET state` record, for the server, the tool and the tests.
- The digest has a recipe prefix (`1:`); `model` carries its own SHA-256 so the status function is synchronous and imports nothing.
- The mark's `commit` is optional; the page has three cases for a changed text, and a checkout older than the verdict prints `checkout_behind`.
- `receipt.ref` is always the API URL of the newest `labeled` event that set `verdict:take`. `plan start` commits the receipt and replaces an earlier one.
- The receipt's digest is compared only in a diff that adds `built` or changes `done`, so a reword never blocks.
- New keys: `tests` (the proof `built` must name) and `done` (owner question 5 needs a home for "command line done"; `[]` is valid).
- After a merge that leaves a feature unfinished the orchestrator runs `plan stop`; `started_unfinished` stops, in the gate, a merge that says neither `built` nor `done`.
- Work that is not a feature is a feature in an area with `"product": false`, listed under `work` in `plan/order.json`, with its own position; `needs` may cross the two lists.
- A decision has no `date` key; `about` is an object in decisions and in question marks, and the flag is typed (`feature:<id>`); `replaces` is a list; `doc` names a design document; `recorded` says the quote is the agent's.
- No quote of the owner is posted: `plan decide` takes `--note`, `plan answer` takes `--outcome`.
- A question's closing comment is the closer's newest comment before the close or within 60 seconds after it; the closer must be a decider or the bot.
- A second record for one question is allowed only when it replaces a `by_default` record.
- A question without a default blocks the features it is about; this is how a step only the owner can do stops a build.
- Suggestions: the label `workshop:seen` and `plan triage`, so "seen" needs no cursor.
- Commands added: `plan decision`, `plan carry`, `plan legacy`, `plan format`, `plan triage`, `plan ask --file`. Only `plan start` commits. Only `plan decide` and `plan start` depend on the branch they run on.
- The tool keeps its own writes for ten minutes at most and uses them until the site has caught up; every command prints the age of what it read.
- Live checks read the refs `main` and `origin/main`, so `plan status` and `plan check --live` agree from any checkout.
- A plan branch is merged with `main`, never rebased, and fast-forwarded into `main` by the orchestrator; it is the one exception to rule 4b.
- A change of areas, of an area's `product` or of `plan/schema/` is `lane: code`. No test reads the real `plan/`, which is what makes the lane safe.
- Of two issues with one marker the lowest wins; one that carries a verdict label is never closed before `plan decide` has run again.
- A label is never renamed or deleted once the first issue exists.
- `--ref` is accepted only while `origin/main` has no `plan/order.json`; this replaces a go-live flag.
- "N of M features are planned" is defined once (section 5); the home page fills it at view time and shows none while live state is unknown.
- `docs/product/RESTART.md` is removed at the cutover, not rewritten: each of its parts has a home in section 2. `README.md` keeps one line and a link.
- Until the cutover the build plugins read the old files through the converter, in memory, so readers switch before `plan/` exists and nothing has two homes. Until go-live the page shows `unknown`.
- Item 0 widens the allowed paths before anything is built, and changes no other rule.
- That a worker changes only `built`, `done`, `tests` and `recordings` is a rule, not a check: a plan change that needs code (a new area) must be able to travel with it.
- The restart tags the last commit of the old `main`, so the commits named by verdict marks stay readable.
- Recordings are limited to 10 MB each. Canonical JSON is `JSON.stringify(value, null, 2)` in schema key order; objects keyed by ids keep their order.
