# The Workshop

The Workshop is the public page of the website at
[silo.amontlabs.com/workshop](https://silo.amontlabs.com/workshop). It shows
Silo's plan, part by part: a map of the product's areas, each with its entries.
Anyone signed in with GitHub can vote, comment and suggest a feature, and the
owner decides each entry there.

This document covers the page, then the server side: the API under
`/api/workshop/`, the GitHub App it signs in with, how it is deployed and run
locally, and its security properties.

## The page

`website/workshop/index.html` and `website/src/workshop/`, React and
TypeScript. At build time the `workshopData` plugin of `website/vite.config.ts`
reads `docs/product/features.jsonl`, `areas.json`, `stages.json` and
`DECISIONS.md` (`data.ts`). A malformed line, an unknown area, an entry without
an area outside the stage `gone`, a repeated id or an area with no entry stops
the build with the file and line. `map.ts` holds the drawing: where each area
sits on the full map and on the folded one, and the wires. The build also stops
when an area has no place there or the drawing places an unknown area; a new
area needs a box in both tables.

What a person sees:

- At 1200 px and wider, the map of the areas (Map), or the same areas as tiles
  in the order of `areas.json` (Order). Narrower, the tiles only; opening a
  tile there is a view transition where the browser has them.
- Each area's state comes from its entries (`plan.ts`, first match wins):
  being built now, done, next (the lowest-numbered planned entry among areas not
  being built), planned, not decided yet. An entry is being built when it is
  planned and its open issue has the label `workshop:building`.
- Opening an area folds the map to a strip that stays on screen (when the
  window is at least 760 px tall); the area's entries open in place, each on
  its own. Opening or closing an entry never moves it or the rows above it. The
  entry opened last is the one in the address and the one a verdict goes to.
  Comments load when a pointer, a press or focus reaches the row, and on every
  opening. The row shows itself open at once; its panel grows when the comments
  are in or after 180 ms, and while they still load each one the count announces
  holds its place. An entry named
  by the address opens once the live state is there.
- The strip under the map links to what is left out (entries without an area,
  and every dropped entry) and to the suggestions.
- A person who can decide also gets "Decisions", the number of entries still to
  decide or to confirm (a verdict carried over from the file) on each part, and
  a link to the lowest-numbered one. After a verdict the next entry waiting in
  the same area opens, else the lowest one anywhere. Each open row has its own
  700 ms guard, and its verdict buttons look dimmed until it has passed. Enter
  goes to the open row that holds the focus; with the focus on the page, to the
  row opened last when it is in view. Verdicts are for entries only: the
  server refuses them on anything else.
  Nobody else is offered the decisions; their address shows the map.
- The header's account slot has a fixed width from first paint: signed in when
  the readable `workshop-hint` cookie is there (a long login is cut by an
  ellipsis), signed out otherwise. It changes only when that cookie was stale.

Addresses: `#<area>`, `#<area>/<entry id>`, `#suggested`, `#left` and
`#decisions`, each with an optional open row (`#suggested/issue-<n>`). An entry
is found by its id whatever area the address names, and the address is
rewritten; an entry that no longer exists opens its area. The older
`#entry-<id>`, `#issue-<n>` and `#decided-<i>` open that row where it now lives.
Back and Forward move between places; opening a row replaces the address.

| File | What |
| --- | --- |
| `App.tsx` | Places, addresses, focus, keyboard, the layout. |
| `useWorkshop.ts` | Live state and every write: pending writes, the verdict guard. |
| `MapView.tsx`, `Tiles.tsx` | The map and the tiles. |
| `AreaView.tsx`, `Suggest.tsx`, `Decisions.tsx` | An opened area, the suggestions and what is not carried over, the decisions. |
| `Entry.tsx` | One row and what it shows open. |
| `routes.ts`, `plan.ts`, `map.ts`, `data.ts` | Addresses, the model, the drawing, the build-time reader. |
| `api.ts`, `writes.ts`, `RichText.tsx`, `Boundary.tsx` | The API client, the six-minute memory of the page's own writes, text from GitHub, the fallback list. |

To look at a later state without the API, run `npm --prefix website run dev`
and open `http://127.0.0.1:4173/workshop/?fixture=later` (some areas done, one
being built, one next) or `?fixture=today`; add `&as=owner` or `&as=visitor` to
be signed in (`&login=<name>` for another login), `&delay=2000` to hold the live state and the sign-in answer back,
and `&detail=700` to hold each entry's comments back. This exists only in
development builds (`devFixture.ts`, loaded when `import.meta.env.DEV`); a
production build has none of it.

## Storage

GitHub issues of `amontlabs/silo` are the only storage. There is no database,
and nothing is written at run time apart from GitHub issues.

| Label | What the issue is |
| --- | --- |
| `workshop:entry` | One line of `docs/product/features.jsonl`. The body ends with `<!-- silo-entry: <id> -->`. |
| `workshop:decision` | A question for the owner. Open: to decide. Closed as completed: decided, and the owner's closing comment is the outcome. |
| `workshop:suggestion` | A feature suggested by a visitor. |

The owner's verdict on an entry is one label: `verdict:take`, `verdict:refine`
or `verdict:later`. A dropped entry has `verdict:drop` and is closed as "not
planned". A built entry is closed as "completed". Votes are 👍 reactions,
comments are issue comments, and history is GitHub's.

## The API

All endpoints are under `/api/workshop/` on the site's own origin and answer
JSON. The shared contract (SHARED.md with its review changes) is restated
briefly here:

| Endpoint | Who | What |
| --- | --- | --- |
| `GET state` | anyone | Every issue with a Workshop label, open or closed: kind, entry id, title, state, verdict, 👍 count, comment count, author, dates. |
| `GET me` | signed in | `{ login, avatar, canDecide }`; `canDecide` is false when the permission could not be read (the next `me` tries again). 401, with the `workshop-hint` cookie cleared, only when there is no valid session. |
| `GET issue?n=` | anyone | Body text (first 20,000 characters), 👍 count from the issue's reaction summary, and the latest 100 comments held, oldest first, as plain text; `more: true` when GitHub counts more. The same public answer for everyone. |
| `GET vote?n=` | signed in | `{ on }`: whether the person has given a 👍, read with their own token. |
| `GET login?return=` | anyone | Redirects to GitHub. `return` must be a path under `/workshop`. |
| `GET callback` | GitHub | Finishes sign-in and redirects to the return path. |
| `POST logout` | anyone | Revokes the person's whole grant for the App (all their sessions), then clears the cookies whatever happened. Answers `{ "ok": true, "revoked": true\|false }`. |
| `POST comment {n, text}` | signed in | Comments as that person. |
| `POST vote {n, on}` | signed in | Adds or removes that person's 👍. |
| `POST suggest {title, text}` | signed in | Opens an issue as that person and labels it `workshop:suggestion`. Returns `{ number }`. |
| `POST verdict {n, verdict, note?}` | can decide | Sets (`take`, `refine`, `later`, `drop`) or clears (`null`) the verdict of an entry; the `verdict` key is required. `refine` needs a note; the labels and the state change first, and the note is posted last, as the person's comment. |
| `POST decide {n, text}` | can decide | Comments on an open decision, then closes it as completed. A comment, a note or a decision text that the same person posted on that issue in the last five minutes is not posted again. |

Errors are `{ "error": code, "message": plain words }` with 400 (bad input),
401 (not signed in, or the session is gone), 403 (not allowed, or a request
from another site), 404 (not a Workshop issue), 405, 413 (body over 64 KB),
429 (GitHub's rate limit or one of ours), 502 (GitHub failed or took over 8
seconds) and 503 `not_configured` (missing or invalid App settings). A
suggestion whose issue was created but could not be labelled answers 502 with
`"error": "github"` and the issue's `number`. `login` and `callback` never
answer JSON: any failure redirects to `/workshop?signin=failed`; cancelling on
GitHub returns to the page.

Each comment is cut to 5,000 characters and then carries `"cut": true`.
Bodies and comments are cut before any processing, and the entry marker and
trailing blank lines are removed with linear string code, so a hostile body
cannot make a read slow.

An issue is a Workshop issue when it carries exactly one of the three kind
labels and is not a pull request.

Comments, notes and decisions are limited to 5,000 characters, titles to 120
characters on one line (no control characters or line breaks). Unicode
direction controls (U+202A–U+202E, U+2066–U+2069) are removed from titles and
text.

### The snapshot: a visitor never causes a call to GitHub

`GET state` and `GET issue` are answered from one in-memory snapshot per
function instance (`website/server/snapshot.mjs`) and nothing else. The
snapshot holds the newest Workshop issues (at most 2,000) with the fields
`state` returns, their bodies (cut to 20,000 characters) and their comments
(the latest 100 per issue, each cut to 5,000 characters, at most 5,000 in
all). Nothing GitHub sends is kept raw: from each list page only its ETag and
the reduced fields of Workshop issues remain, and from comment pages only the
cut comments of Workshop issues; every kept text is a fresh copy of its cut,
never a slice that would keep GitHub's whole text alive. A number that is not in the snapshot is a
404. Both reads are sent with
`Cache-Control: public, s-maxage=10, stale-while-revalidate=60`, no
`Vary: Cookie`, and read no cookie.

**What a read costs.** Nothing: no `state` or `issue` request calls GitHub.
A read may start a refresh, which runs on its own ceiling.

**How soon a change is seen.** The person who wrote sees their change at once:
the page shows it itself. Others on the same instance see it within a second
or two, from the patch and then the refresh the write starts. Everyone else,
including a new suggestion on other instances, sees it within about 40
seconds: up to 10 seconds of CDN cache plus the 30-second refresh of the other
instances.

- **Refresh.** A request that finds the snapshot over 30 seconds old starts a
  refresh and waits for it up to 1.5 seconds before answering from what it
  has. Any request that changed something on GitHub (a comment, a vote, a
  suggestion, a verdict, a decision, even one that failed halfway) starts one
  straight away; when it succeeded, it waits for that refresh up to 2 seconds
  (the page already shows the write as pending), but only when the refresh is
  running or will start within those 2 seconds; otherwise it answers at once
  and the refresh comes when the floor allows. Refreshes are coalesced: at
  most one runs and one waits; a write that lands while one is running makes
  exactly one more start when it ends, so the write is always included; a
  refresh asked for by a write starts at most once every 5 seconds per
  instance. No refresh starts while GitHub has asked us to wait. Each refresh
  has its own 12-second limit and an abort signal: past the limit, or when it
  is declared lost (unfinished after 15 seconds), its calls are aborted and it
  stops spending, so two never spend at once; the next request starts a new
  one. A failure that arrives more than 15 seconds after its refresh started
  (an instance that was frozen) does not set the 60-second wait. Only an instance with
  no snapshot yet makes its first requests wait for the refresh, at most until
  their 12-second deadline. A refresh builds a new snapshot and swaps it in
  only when it completed, so a refresh that fails halfway leaves the previous
  one intact.
- **Staying alive.** Work that outlives the response (refreshes and the label
  retries) is handed to Vercel's `waitUntil()` from `@vercel/functions`, which
  "extends the lifetime of the request handler for the lifetime of the given
  Promise"; such promises "have the same timeout as the function itself"
  ([@vercel/functions API reference](https://vercel.com/docs/functions/functions-api-reference/vercel-functions-package),
  read 2026-11-16). Correctness does not depend on it: the lost-refresh rule
  and the waits above cover a platform that stops the work.
- **Issues.** A refresh reads one list per Workshop label: entries, then
  decisions, then suggestions (`labels=workshop:…`, all states, newest first,
  100 a page, at most 20 pages for the three together), so nothing else in the
  repository is fetched or counted against the cap, and issues without exactly
  one Workshop label or that are pull requests are skipped.
  Each page is sent with `If-None-Match` and the ETag GitHub gave for it last
  time; a `304 Not Modified` means the page is unchanged and the held copy is
  reused. Vote counts come from each issue's reaction summary in that list.
- **Comments.** The first refresh, and one every 10 minutes, reloads the
  newest comments of the whole repository (`/issues/comments`, newest first,
  at most 15 pages, also conditional), so deleted comments disappear within
  10 minutes; comments older than those 1,500 are not held, and `more` is true
  for their issues. The refreshes in between ask only for comments updated
  since the previous refresh, less a minute of overlap (`since`, oldest update
  first, at most 10 pages; more than that schedules a full reload), and merge
  them by id.
- **Writes.** After a signed-in person's write succeeds, the snapshot is
  patched in place: the new comment, the comment and 👍 counts, the verdict,
  open or closed (a drop sets `closedAt`), a new suggestion. Each patch is kept
  with its time. When a refresh completes, GitHub's data replaces the snapshot
  and the patches made after that refresh began are applied again; older
  patches are dropped, since GitHub's answer already holds them. A count moves
  only from the value it had when the write happened, so a write GitHub
  already shows is not counted twice.
- **When GitHub fails**, times out or asks us to wait, the snapshot keeps
  answering whatever its age, with `"stale": true` once it is over 10 minutes
  old, and no refresh starts before the time GitHub gave, or 60 seconds. With
  no snapshot at all, both reads answer 502 and the page falls back to the
  plan built into it. When the App is not installed on the repository, `state`
  and `issue` answer 503 `not_configured`.
- **Output.** Titles, bodies, comments and logins in `state` and `issue` are
  cleaned on the way out: no direction-override or control characters, and no
  line breaks in titles and logins.

GitHub documents that a `304` answer to an authorized conditional request
"does not count against your primary rate limit"
([best practices for using the REST API](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api),
read 2026-11-16). An unchanged issue page therefore costs nothing against the
installation's 5,000 requests an hour
([rate limits for the REST API](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)).
It still counts in our own ceiling below. The `since` comment call has a new
URL each time and always counts.

### The installation token and its ceilings

The installation token is spent only on the snapshot refresh, the permission
check, the repository lookup at sign-in and labelling a suggestion. Each has
its own ceiling per instance, so that no use can starve another, and nothing
a visitor does without signing in can make `verdict`, `decide`, `suggest`,
`comment`, `vote` or `me` answer 429:

| Use | Calls a minute |
| --- | --- |
| Snapshot refresh | 40 |
| Permission check for accounts not known to decide | 12 |
| Permission re-read for accounts known to decide (nobody else can spend it) | 4 |
| Repository lookup at sign-in (once per instance, then remembered) | 4 |
| Labelling a suggestion, and the label retries (charged per call) | 20 |

Worst case per instance per hour: (40 + 12 + 4 + 4 + 20) × 60 = 80 × 60 =
4,800 calls, below the installation's 5,000. The largest refresh is 20 issue
pages + 15 comment pages = 35 calls, which fits in a minute's 40, and 80 calls
a minute is far below GitHub's secondary limit of 900 points a minute for
`GET` requests. Calls to GitHub's token endpoint are not installation calls;
new sign-ins have a ceiling of 30 a minute and token refreshes of existing
sessions a separate one of 60, so made-up sign-ins cannot stop sessions from
refreshing.

With each of the three lists on one page, a refresh is 4 calls (3 list pages,
which are `304` and free when unchanged, and 1 `since` comment call, which
always counts against GitHub's limit):

- **No writes:** one refresh every 30 seconds, 120 an hour: 120 × 4 = 480
  calls in our ceiling, of which about 120 count at GitHub, plus 6 full
  comment reloads whose unchanged pages are free: about 130 calls an hour.
- **Writes without pause:** write-started refreshes are at most one every 5
  seconds, 12 a minute: 12 × 4 = 48 calls a minute, which the refresh ceiling
  holds to 40 a minute (2,400 an hour). A refresh stopped by our own
  ceiling is started again when the ceiling resets, without the 60-second
  wait used when GitHub fails.

**The limit is shared by every instance.** The 5,000 calls an hour belong to
the installation, and every warm instance spends from them. At about 130 calls
an hour each, about 38 warm instances that are all idle use up the hour;
two instances spending at their ceilings (4,800 an hour each) use it up in
about half an hour. Nothing in this code can prevent that: instances do not
know about each other. When it happens, GitHub answers with a rate limit, every
instance keeps serving its snapshot (stale after 10 minutes), and the
snapshots catch up when the hour resets.

### Other limits

- Everything other than `state` and `issue`, and every response that sets a
  cookie, is `private, no-store`.
- When GitHub rate-limits the App (including a secondary limit: a 403 or 429
  with `retry-after`, or whose message mentions a rate limit), nothing calls
  GitHub with the installation token until the time GitHub gave (at most an
  hour).
- Each signed-in person may make 20 writes a minute per instance; people known
  to be able to decide have their own allowance of 120. Sign-out counts as a
  write. A person may make 3 suggestions a minute and 10 an hour; an attempt
  counts only when the issue was created.
- Each session may make 60 GitHub calls a minute for reads (`GET vote` and the
  reads before a write), or 300 for a person known to be able to decide; more
  answer 429. A verdict and its note are admitted together: the reads the
  whole action needs are checked before its first call.
- A session GitHub has rejected (a 401, an invalid refresh token, a revoked
  grant) is remembered by a hash of its token for a day (at most 10,000) and
  answered 401 without calling GitHub; a request without a valid session calls
  nothing.
- A sign-in cookie works for one callback per instance (its id is remembered
  for its 10-minute life, at most 10,000).
- Every GitHub call has an 8-second limit, and every handler has a 12-second
  deadline for all its GitHub calls and every wait together, including waits
  on shared work such as an installation token another request is fetching
  (the platform stops a function at 15 seconds); past it, the answer is 502.
  Steps are ordered so that a cut is safe to retry: `verdict` changes labels
  and state, then posts the note; `decide` posts the comment, then closes;
  `comment`, the note and `decide` skip posting when the same person posted
  the identical text on that issue in the last five minutes.
- Concurrent permission checks for the same person share one call.
- `me` reads the cookie and calls GitHub only for the permission check (cached
  five minutes); it never refreshes the user token, and a failed permission
  read answers `canDecide: false` rather than an error.
- `suggest` refuses with 429 before creating anything when the installation
  is blocked or the label ceiling is reached. If labelling a new suggestion
  still fails twice, the answer is 502 with its `number`, and the number is
  remembered (in memory, up to 100). The list is retried in the background
  after a successful `suggest` and after each snapshot refresh, never on other
  requests and never before an answer. Each number is read again first and
  labelled only when the issue is still open and has no label at all; anything
  else leaves the list, a failing number does not hold up the others, and a
  number that failed five times is dropped.
- Every answer, API and pages alike, carries
  `Content-Security-Policy: frame-ancestors 'none'` and `X-Frame-Options: DENY`.

## Code

| Path | What |
| --- | --- |
| `website/server/config.mjs` | Settings from the environment. |
| `website/server/github.mjs` | GitHub REST client with pagination and an 8-second limit per call; errors carry a status and the identity used, never a body. |
| `website/server/app-auth.mjs` | App JWT (RS256, Node's `crypto`) and the cached installation token. |
| `website/server/session.mjs` | Sealed cookies (AES-256-GCM). |
| `website/server/issues.mjs` | Kinds, verdicts, the entry marker and the cuts. |
| `website/server/snapshot.mjs` | The in-memory snapshot that answers every anonymous read, and its refresh. |
| `website/server/workshop.mjs` | Every endpoint, independent of the HTTP server. |
| `website/server/node.mjs` | Adapter for Node's `req`/`res`, used by Vercel and the local server. |
| `website/api/workshop.cjs` | Vercel's entry point. |
| `website/scripts/prepare-vercel.mjs` | Writes the Build Output (static site, function, routes). |
| `website/scripts/workshop-dev.mjs` | Local server for the site and the API. |
| `website/scripts/fake-github.mjs` | Fake GitHub used by the tests and by `--fake`. |
| `website/scripts/workshop-test-support.mjs` | Shared test setup: the fake GitHub on a controllable clock and a client with a cookie jar. |
| `website/scripts/workshop-server.test.mjs` | Endpoints, sign-in, sessions, refusals, configuration, packaging and the local server. |
| `website/scripts/workshop-snapshot.test.mjs` | The snapshot: reads without GitHub calls, conditional refreshes, changes, patches, bounds. |
| `website/scripts/workshop-limits.test.mjs` | Ceilings, `me`, sign-out, deadlines, patch timing, cleaned output, the App not installed. |
| `website/scripts/workshop-refresh.test.mjs` | Refreshes started by writes, coalescing, the floor and the idle refresh. |
| `website/scripts/workshop-abuse.test.mjs` | What a visitor or a replayed cookie can make the server do, memory, waits and retries. |

All of them run with the website's `npm test`.

The server uses Node's standard library (`crypto`, `fetch`, `http`) and one
package, `@vercel/functions` (Apache-2.0, pinned to 3.10.5), for `waitUntil`.
`prepare-vercel.mjs` ships only the files of it that load (found by loading it
in a separate process) and its `package.json`: 16 files, and the whole
function is about 180 KB.

## The GitHub App

Create it under the `amontlabs` organization (Settings → Developer settings →
GitHub Apps → New GitHub App):

| Setting | Value |
| --- | --- |
| Name | Silo Workshop (posts show as `silo-workshop[bot]`) |
| Homepage URL | `https://silo.amontlabs.com/workshop` |
| Callback URL | `https://silo.amontlabs.com/api/workshop/callback` (the only one) |
| Expire user authorization tokens | On |
| Request user authorization (OAuth) during installation | Off |
| Enable Device Flow | Off |
| Webhook | Inactive |
| Repository permissions | Issues: Read and write. Metadata: Read-only. Nothing else. |
| Organization and account permissions | None |
| Where can this GitHub App be installed | Any account (public) |

The App must be public: GitHub lets only members of the owning organization
authorize a private App, so a private one would stop every visitor from signing
in. Being public means anyone may install the App on their own repositories.
That gives them nothing: the server only ever uses the installation on
`WORKSHOP_REPO`, and it requests user tokens for that repository only.

Then generate a private key and a client secret, and install the App on
`amontlabs/silo` only ("Only select repositories"). Create the seven labels in
the repository: `workshop:entry`, `workshop:decision`, `workshop:suggestion`,
`verdict:take`, `verdict:refine`, `verdict:later`, `verdict:drop`.

For local work against the real GitHub, create a second App the same way with
the callback `http://127.0.0.1:4174/api/workshop/callback`, installed on a test
repository. Do not add a local callback to the production App.

## Environment variables

Set them in the Vercel project (Settings → Environment Variables) for the
**Production** environment only, marked Sensitive. Preview deployments are
built from any branch and must not receive the App's secrets; without them
the API answers 503 and the page shows the plan without live data.

| Variable | Value |
| --- | --- |
| `WORKSHOP_APP_ID` | The App's numeric ID. |
| `WORKSHOP_CLIENT_ID` | The App's client ID. |
| `WORKSHOP_CLIENT_SECRET` | A client secret of the App. |
| `WORKSHOP_PRIVATE_KEY` | The App's private key, PEM. Newlines may be written as `\n`. |
| `WORKSHOP_SESSION_SECRET` | Exactly 32 random bytes in standard base64: `openssl rand -base64 32`. Changing it signs everyone out. |
| `WORKSHOP_REPO` | Optional, default `amontlabs/silo`. |
| `WORKSHOP_SITE_ORIGIN` | Optional, default `https://silo.amontlabs.com`: a bare origin, no path. Writes are accepted only from this origin, and cookies are `__Host-` and `Secure` only when it is `https`. |

A missing or invalid value (a secret that is not 32 bytes of base64, a key that is not an RSA private key, a malformed repository or origin) leaves the API answering 503 `not_configured`. The response never says which value is wrong; the server log names the variable, never its value.


## Deployment

`npm --prefix website run build:vercel` builds the site and runs
`prepare-vercel.mjs`, which writes `.vercel/output/` at the repository root
following Vercel's
[Build Output API v3](https://vercel.com/docs/build-output-api/configuration):

- `static/`: the built site.
- `functions/api/workshop/state.func/`: one Node.js function
  (`.vc-config.json`: runtime `nodejs24.x`, handler `api/workshop.cjs`,
  launcher `Nodejs`), holding the entry point and `server/`. The other
  endpoints (`me.func`, `issue.func`, `vote.func`, …) are symlinks to it, so
  Vercel creates one function served at every `/api/workshop/<endpoint>` path,
  as the [primitives reference](https://vercel.com/docs/build-output-api/primitives#functions)
  allows. An unknown endpoint is Vercel's 404.
- `config.json`: `version: 3` and three routes. The first adds
  `Content-Security-Policy: frame-ancestors 'none'` and `X-Frame-Options: DENY`
  to every path and continues. The second,
  `{ "src": "^/workshop/?$", "dest": "/workshop/index.html" }`, serves the
  Workshop page at both `/workshop` and `/workshop/`. The third hands every
  other path to the filesystem.

The script fails when the build has no Workshop page (`workshop/index.html`, or
`workshop.html`). `--out <dir>` writes the output elsewhere; the script refuses
any directory whose path does not end in `.vercel/output`, so it never deletes
anything outside such a directory. Both
`/.vercel` and `website/.vercel` are ignored by Git.

The Vercel project's root directory is the repository root, where Vercel reads
`.vercel/output` after the build. The one project setting this needs is the
build command: `npm --prefix website run build:vercel` (it was
`npm --prefix website run build`, which produces only the static site in
`website/dist`).

## Running locally

```sh
npm --prefix website run build
npm --prefix website run workshop:dev -- --fake
```

This serves `website/dist` and the API at `http://127.0.0.1:4174`
(`--port` to change it), with the Workshop page at `/workshop` and
`/workshop/`. `--fake` starts a fake GitHub on another local port, seeded with
an entry for every line of `docs/product/features.jsonl`, two decisions (one
open), a suggestion, comments, 👍 votes, an ordinary issue and a pull request.
Sign-in goes through the fake's page, which offers two people: `owner-fake`
(admin, can decide) and `visitor-fake` (read only). Everything lives in memory
and resets on restart.

Without `--fake`, the server uses GitHub with the `WORKSHOP_*` variables of the
environment (a development App, see above); its origin is always
`http://127.0.0.1:<port>`, so cookies are not `Secure` and have no `__Host-`
prefix. Without the variables, the API answers 503.

The tests run with the website's tests: `npm --prefix website test`. The fake
GitHub answers like GitHub where it matters to the server: expiring user tokens
with PKCE, a single-use refresh token, grant revocation, the permission
endpoint answering 200 with `none` for a non-collaborator, ETags and `304` on
listings, the repository-wide comment list with `sort`, `direction` and
`since`, comment edits and deletions, primary and
secondary rate limits, locked issues, and a hook that delays or fails any call.

## Security properties and their limits

**Session.** One cookie, `__Host-workshop-session` (`Path=/`, no `Domain`,
`HttpOnly`, `Secure`, `SameSite=Lax`, 30 days). It holds the person's GitHub
id, login, avatar, user token, refresh token and their expiries, sealed with
AES-256-GCM under a key derived (HKDF-SHA256) from `WORKSHOP_SESSION_SECRET`
for this purpose only. A changed byte, another key or a value sealed for the
sign-in cookie does not open. The session ends 30 days after sign-in. The user
token is refreshed when under five minutes remain. The session is cleared only
when GitHub says the refresh token is invalid, or when a call with the person's
token answers 401; any other failure answers 502 and keeps the cookie. A
readable marker cookie, `workshop-hint=1` (no secret, same lifetime, not
`HttpOnly`), tells the page to ask `me`; it is set at sign-in and cleared with
the session.

*Limits:* there is no server-side session list. `me` reads only the cookie, so
after a sign-out elsewhere it keeps answering until the next call made with
the person's token is refused (401, and the session is cleared). A stolen
cookie works until it expires, the person signs out, or the secret is changed.

**Sign-out.** Sign-out revokes the person's whole grant for the App (all their
sessions), then clears the cookies. It uses the token the cookie holds and
never waits on a refresh. The answer's `revoked` says whether GitHub confirmed
the revocation. When it is false, the grant may still be active on GitHub; a
person revokes it by hand on GitHub under Settings → Applications → Authorized
GitHub Apps → Silo Workshop → Revoke. The cookies are cleared in every case.

**Sign-in.** `login` sets a 10-minute sealed `__Host-workshop-signin` cookie
holding a random `state`, a PKCE verifier (S256) and the return path.
`callback` clears it, compares `state` in constant time, and exchanges the code
with the verifier and `repository_id`, so the user token works only on the
Workshop repository. The redirect URI is fixed to the configured origin. A
token without expiry (the App setting off) is refused. `return` is accepted
only as a path under `/workshop` (no scheme, no `//`, no backslash, no control
characters; dot segments are resolved and checked).

**Cross-site requests.** Every `POST` needs `Origin` equal to
`WORKSHOP_SITE_ORIGIN` and the header `X-Workshop: 1`, which a cross-site page
cannot send without a CORS preflight that the server never approves (it sends
no CORS headers). The cookies are `SameSite=Lax`, so cross-site subrequests do
not carry them. Writes from preview deployments are refused because their
origin differs.

**Identities and scope.** The App can only read and write issues of the one
repository, and the installation token is requested for that repository with
those two permissions. The installation token serves only the snapshot refresh,
the permission check and labelling a suggestion
(which a visitor cannot do). Everything else done for a signed-in person,
including reading the issue before a write, uses that person's token, so
GitHub's own permissions apply (a locked issue refuses a visitor's comment)
and GitHub records the person as the author. Every write first reads the issue
and refuses with 404 unless it carries a Workshop label and is not a pull
request; `verdict` works only on entries and `decide` only on open decisions.
A verdict adds and removes only `verdict:*` labels, one at a time, so labels
changed by someone else are kept. It changes the labels and the state first
and posts the note only after they succeeded, so a retry never posts the note
twice. Clearing a verdict reopens only a dropped entry, never one closed as
completed. A 401 on the installation token is a 502 and
drops the cached token; it never signs anyone out. *Limit:* if labelling a
suggestion fails twice, the new issue stays outside the Workshop until a
background retry labels it (in memory, on that instance only), and the answer
carries its number.

**Deciding.** `canDecide` is GitHub's answer: the person's permission on the
repository (`write`, `maintain` or `admin`), read with the installation token
and cached five minutes by GitHub account id, for yes and for no; that one
cache serves `me`, `verdict` and `decide`. The permission is read for the
account's current login, asked of GitHub with the person's own token at that
moment, never the login in the cookie, and the answer must be about the same
account id as the session. Writes are not preceded by a re-read: they use the
person's own token, and GitHub itself refuses a label change or a close by
someone without access; such a refusal (403) drops that person's cached yes,
and only a yes. Accounts known to be able to decide re-read on an allowance
nobody else's requests can spend; a read that says no removes the account
from them.
Reading the permission with the person's own token (`GET /repos/{owner}/{repo}`
and its `permissions` object) was considered: GitHub's reference for that
endpoint lists `permissions` in the response schema but does not say which
kinds of token receive it
([Get a repository](https://docs.github.com/en/rest/repos/repos#get-a-repository),
read 2026-11-16), so the documented collaborator permission endpoint is used.

**Secrets in output.** Logs are one JSON line per request with the endpoint,
the status and GitHub's status only. Errors sent to the browser are fixed
sentences; nothing from a GitHub response body is passed on. No token, code,
cookie, client secret or private key is logged or returned; a test asserts
this on every response and log line of a session that includes failures.

**Rate limits.** The snapshot, the ceilings and the caches are per function
instance, in memory; several instances together can make more calls than one,
and two instances can briefly show different versions.

## Not verified until deployed

Everything above was run against the fake GitHub and in Node, not against
GitHub or Vercel. These depend on the real services:

- the token exchange body format: the server sends JSON with
  `Accept: application/json`, including `code_verifier` and `repository_id`;
- whether a refreshed user token stays limited to the repository;
- a visitor removing their own 👍 with their user token;
- the collaborator permission endpoint with only the Metadata read permission;
- that Vercel follows the symlinked functions, and loads the `.cjs` handler
  with `req.url` as the original path;
- that Vercel's Git build uses `.vercel/output` at the repository root;
- that `waitUntil()` from the copied `@vercel/functions` keeps a Build Output
  Node.js function alive after the response, as documented for Vercel
  Functions (the code does not depend on it);
- that GitHub answers `304` to the conditional issue and comment list pages as
  documented, and that the reaction summary in the issue list moves the page's
  ETag when a vote changes.

## Why built this way

- **giscus** stores comments in GitHub Discussions and has no place for a
  verdict, a vote that the owner's page counts, or a decision's state.
- **utterances** signs people in with an OAuth App that needs the broad
  `public_repo` scope, write access to every public repository of the person.
  A GitHub App's user token is limited to the App's permissions and, here, to
  one repository.
- **A database of our own** is something to run, secure and back up. Issues
  keep the history, the authorship and the moderation tools on GitHub.
- **Verdicts as commits** to `docs/product/features.jsonl` would put a commit
  on `main` for every click, which interrupts a merge in progress. The file
  keeps carried-over verdicts; new ones are labels.
