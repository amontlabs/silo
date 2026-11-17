# Silo website

The public site at [silo.amontlabs.com](https://silo.amontlabs.com), built with
Vite. It has two pages:

- **Home** (`index.html`, `src/style.css`, `src/loop.ts`): the pitch, the
  picture of the loop (drawn by `src/loop.ts`, once, when it comes into view,
  unless the visitor prefers reduced motion), who Silo is for, the way into the
  Workshop, and a notice that Silo is in heavy development. Nothing on it is
  read from the plan. The page shows once its fonts are in (at most 1.2 s), so
  nothing changes width: it preloads every face it waits for, and the deployed
  site lets a browser keep the font files, so a later visit does not wait.
  A link to the site shows `public/og.png` when shared: a 1200 × 630 capture
  of `scripts/share-image.html`, opened from the dev server at
  `/scripts/share-image.html`.
- **Workshop** (`workshop/index.html`, `src/workshop/`), at
  [silo.amontlabs.com/workshop](https://silo.amontlabs.com/workshop): Silo's
  plan, the product's areas drawn as a map (tiles on narrow screens), each with
  its entries, votes, comments, suggestions and the owner's verdicts. Anyone
  signed in with GitHub can vote, comment and suggest a feature, and the owner
  decides each entry there. Its API lives under `/api/workshop/` (`server/`,
  `api/`).

Shared files: `src/site.css` (tokens, the gutter, the frame and its diamonds,
the header, buttons, the theme switch and the drawn arrow and check icons),
`public/theme.js` (first-paint light, dark or System theme,
remembered in local storage), `public/fonts/` (self-hosted IBM Plex under the
included SIL Open Font License) and `public/favicon.svg`.

This document covers running and testing the site and the Workshop page, then
the Workshop's server side: its storage, the API, the code, deployment, the
GitHub App it signs in with, how it goes live, its security properties and what
is not verified until deployed.

## Run locally

Use Node.js 26. From the repository root:

```sh
npm --prefix website ci
npm --prefix website run dev
```

The page preview is at http://127.0.0.1:4173. The Workshop there has no API.
To look at a later state without the API, open
`http://127.0.0.1:4173/workshop/?fixture=later` (verdicts with their
history lines, two entries being built) or `?fixture=today`; add `&as=owner` or `&as=visitor` to
be signed in (`&login=<name>` for another login), `&delay=2000` to hold the live state and the sign-in answer back,
and `&detail=700` to hold each entry's comments back. The page's memory of the last visit is off with a fixture,
so `&delay=` shows a first visit; `&memory=1` turns it on. This exists only in
development builds (`devFixture.ts`, loaded when `import.meta.env.DEV`); a
production build has none of it.

To run the site with its API against a fake GitHub:

```sh
npm --prefix website run build
npm --prefix website run workshop:dev -- --fake
```

This serves `website/dist` and the API at `http://127.0.0.1:4174`
(`--port` to change it), with the Workshop page at `/workshop` and
`/workshop/`. `--fake` starts a fake GitHub on another local port, seeded with
an entry for every feature of `plan/`, two issues with the retired label
`workshop:decision` (outside the Workshop), a suggestion, comments, 👍 votes,
an ordinary issue and a pull request. It also serves the files of the
repository at a commit as `raw.githubusercontent.com` does, and the runner
names it to the page (`<meta name="workshop-raw">`), so "Text changed" opens.
Sign-in goes through the fake's page, which offers two people: `owner-fake`
(admin, can decide) and `visitor-fake` (read only). Everything lives in memory
and resets on restart.

`--fake --progress` seeds a later state instead: the entries of the first five
stages carry the verdict of their file, else the one suggested for them, the
first two taken are being built, and the thread of each decided entry holds the
bot's history comments of every kind beside people's comments over the last
days. Nothing shows as built that the file does not say is built (the page
reads `built` from the file), and every history line agrees with the state the
sample shows: "Built" only on an entry built, and an entry's last verdict line
is its verdict.
The runner prints what it seeded.

Without `--fake`, the server uses GitHub with the `WORKSHOP_*` variables of the
environment (a development App, see [The GitHub App](#the-github-app)); its origin is always
`http://127.0.0.1:<port>`, so cookies are not `Secure` and have no `__Host-`
prefix. Without the variables, the API answers 503.

To check how the Workshop loads in a real browser, with the server above
running:

```sh
npx --prefix website playwright-core install chromium-headless-shell
npm --prefix website run load-check
```

The first command downloads Playwright's own Chromium once; the check never
uses an installed Chrome. It opens `http://127.0.0.1:4174/workshop` (pass
another URL after `--` to check another server) at 1440 × 900, 1440 × 1100,
2560 × 1440, 1024 × 1366 and 390 × 844, on the plain view, the first area,
`#latest` and `#suggested`. For each load it prints when the page first
painted, when the account, the votes, the owner's marks and "Suggest a
feature" appeared, what the page's first frame held, and every layout shift
with their total. Every one of these loads is a first visit: its first frame
must hold the sign-in button (unless `--owner`), "Suggest a feature" on the
plain view, and a vote placeholder on every row of the first area. Then, in one
browser profile at 1440 × 900, the first area is visited twice (the second
visit must have the votes in its first frame), and the owner signs in through
the fake GitHub's page as `owner-fake` (so the session cookie, its marker and
the remembered person are real) and visits again: the login, "N to decide"
and the rows' "to decide" must be in its first frame (the app's first frame:
the markup the build drew does not count). Then an entry of the
first area without comments is opened before the answers, at 1440 × 900 and
390 × 844, and the rows below it must not move when they arrive; and
`#suggested` is visited signed in with nobody remembered, and the list must not
move when `me` answers. "account" is when the login or the sign-in button
appeared, not its placeholder. It checks that the
footer is the last row of the frame, at the bottom of the window when the page
is shorter than the window and right after the content otherwise, once before
the page's script runs and once after everything has loaded. It exits with an
error when that check fails, a first frame lacks what it must hold, or a
load's layout shifts add up to more than 0.01 (the key gains an item when the
state arrives, a shift of a few ten-thousandths). `--slow-api <ms>` holds back
each API answer, `--slow-script <ms>` holds back the page's script, and
`--owner` fakes the `me` answer as the owner. Last, with the page's script
held (run it with `--slow-script 3000`) and the API held longer, it loads the
built page at 1440 × 900: on the plain view the first painted frame must hold
the map and the sign-in button, the footer rule must hold, and the title row,
the map, the footer and the account slot must not move and no layout shift
happen when the script takes over; on the first area the plain map must never
be painted and the area must be in the first frame after the takeover; and on a
second visit, the state remembered, nothing under the title row may move by
half a pixel or more when the script takes over, at 1440 × 900 and 390 × 844.
`npm test` does not run it.

## Test

```sh
npm --prefix website run typecheck
npm --prefix website test
```

`npm test` runs the Node tests in `scripts/*.test.mjs` (the theme script, and
the Workshop server against its fake GitHub, including the Vercel output
layout and the local server), then the page's Vitest tests.

The fake GitHub answers like GitHub where it matters to the server: expiring user tokens
with PKCE, a single-use refresh token, grant revocation, the permission
endpoint answering 200 with `none` for a non-collaborator, ETags and `304` on
listings, the repository-wide comment list with `sort`, `direction` and
`since`, comment edits and deletions, primary and
secondary rate limits, locked issues, and a hook that delays or fails any call.

## The Workshop page

React and TypeScript. At build time the `workshopData` plugin of `website/vite.config.ts`
reads `plan/` with `tools/plan/read` and checks it with `tools/plan/check`
(`data.ts`); any error of `plan check`, and an area with no feature, stops the
build with the file, the field and the fix. `map.ts` holds the drawing: where each
area sits on the full map and on the folded one, and the wires. The build also
stops when an area has no place there or the drawing places an unknown area; a
new area needs a box in both tables.

`npm run build` then runs `scripts/prerender.mjs`, which renders the plain
address `/workshop` (`src/workshop/prerender.tsx`: nothing remembered, nobody
signed in, the load pending) and writes it into `dist/workshop/index.html`: the
title row, the map, the tiles and the row under them in `#workshop-root`, and
"Sign in with GitHub" with its tooltip in the header's account slot. Both carry
`data-prerendered` until the script takes over. A prerender that throws fails
the build; the dev server does not prerender. Rendering at build time reads
nothing of `window`, `document`, `location`, `localStorage` or `matchMedia`
(effects may). So the page is drawn whole before its script arrives:

- The script takes over in one synchronous step (`takeover.ts`): React renders
  over the markup and commits before the next frame, and the drawn sign-in
  leaves the slot in the same task. On a first visit without a session at the
  plain address the first render is the same markup as the build makes it
  (`prerender.test.tsx` compares them); with something remembered it differs
  on purpose.
- With the session marker (`ws-session` on `<html>`), CSS shows the account's
  placeholder in the slot instead of the drawn sign-in, while the script may
  still take over.
- At any other address (a hash, `?view=order`, `?signin=failed`) the head script
  sets `ws-hold` before first paint: what is under the title row stays hidden
  with its room kept, and the footer with it, until the script has drawn that
  place.
- When the script gives the page no account (a script fails to load, it has not
  taken over after 4 s, or the page fails to render and shows the plain list),
  `ws-bare` goes on `<html>`: everything the build drew shows, and the drawn
  "Sign in with GitHub" replaces the placeholder, marker or not.
- Without JavaScript the plan is readable from the page, and a line under it
  says what needs JavaScript.
- The page preloads every face it waits for and shows as soon as they are in
  (at most 1.2 s), without waiting for its own script: the check runs right
  after the fonts' stylesheet.

Names, labels, markers, the status rule and the areas' states come from
`tools/plan/model/` (`plan.ts` imports them), as in the server: there is no
second definition of them in the site.

What a person sees:

- At 1200 px and wider, the map of the areas (Map), or the same areas as tiles
  in the order of `areas.json` (Order). Narrower, the tiles only; opening a
  tile there is a view transition where the browser has them.
- An entry's status is the model's (`featureStatus`): built when its file says
  `built`; otherwise the verdict that counts (its issue's verdict label when it
  has one, else the file's): dropped, being built (taken, and its open issue has
  the label `workshop:building`), planned, later, not decided.
- Each area's state comes from its entries (the model's `areaStates`, first
  match wins): being built now, done, next (the first planned entry in build
  order among areas not being built), planned, not decided yet.
- Opening an area folds the map to a strip that stays on screen (when the
  window is at least 760 px tall); the area's entries open in place, each on
  its own. Opening or closing an entry never moves it or the rows above it. The
  entry opened last is the one in the address and the one a verdict goes to.
  Comments load when a pointer, a press or focus reaches the row, and on every
  opening. The row shows itself open at once; its panel grows when the comments
  are in or after 180 ms, and while they still load each one the count announces
  holds its place. An entry named
  by the address opens at once when a live state is shown (remembered or
  answered); with none yet, it waits closed until the first answer. An address
  naming something the state shown lacks falls back to the plan only once the
  load has answered or failed, and the `fresh=1` ask has too when one was made.
  The person's own vote on an opened row is checked only once `me` has answered
  on this visit, so a session found gone on page load says nothing. The suggest
  form shows whenever the person is shown signed in and keeps its draft; it
  sends only while a state is shown.
- What the page shows first: the plan is in the bundle. The live state (votes,
  comments, suggestions, the owner's marks) and who is signed in come from
  `GET state` and `GET me`, and the page remembers both in local storage
  (`memory.ts`): the newest answer as the server sent it, and the signed-in
  login, avatar and whether they decide. The page's own confirmed writes are
  kept apart (`writes.ts`), as a list with each one's time, of values set: a
  verdict, a new suggestion, and for a vote or a comment the count the server
  answered for that write (`up`, `comments`; the count shown plus or minus one
  when an older server answers none). Never a change to a count: the server
  patches its snapshot in place, so an answer may already count a write made
  after its `fetchedAt`. What the page shows is the answer with every write applied in
  order, except those the answer was fetched more than a minute after; a write
  is dropped after a day, the page's own comments after six minutes. Both are drawn in the
  first frame. The person is used only while the `workshop-hint` cookie is
  there, and forgotten on sign-out, on a 401 and when `me` answers signed out; a
  state over 7 days old or of another shape is ignored. Without the cookie the
  visitor is signed out from the first frame ("Sign in with GitHub", "Sign in to
  comment", "Sign in to suggest"). What is remembered about the person is for
  display: a verdict is sent only once `me` has answered on this visit that
  they decide, and until then the verdict buttons look dimmed.
- The load: `state` is asked once. Its answer is shown unless the state shown
  is newer (by `fetchedAt`). An old answer (`old: true`, an `Age` header over
  10, or fetched over 2 minutes ago) is followed by one `state?fresh=1`, shown
  in place; a failure of that ask says nothing. A failed first ask is tried
  once more after 1.5 s (not on `not_configured`); then "Votes and comments are
  offline." shows with "Try again", and remembered numbers stay on screen.
- While no state is shown and the first answer is to come, placeholders hold the
  room of what is known to come: the vote of every row, the header's account
  when the cookie is there and nobody is remembered, Latest's lines, an open
  entry's comment box, two rows of suggestions and the suggest form; notes have
  their own while their chunk loads. They pulse slowly (not with reduced
  motion), are `aria-hidden`, and their region is `aria-busy`. Nothing has a
  placeholder that may not come (comment counts, "N suggested", the owner's
  marks). What arrives after the first frames (numbers, the account, the
  owner's marks, a new item of the key) fades in over 150 ms where it stands;
  nothing fades on navigation or when a row opens, or with reduced motion.
- An open entry shows its sentence, then "Notes" (its `<id>.md`), closed by
  default, and opened in place: headings, paragraphs, lists, http and https
  links (an address may hold one level of brackets), inline code and code
  blocks become elements (`Markdown.tsx`); nothing is ever inserted as HTML,
  and anything else stays text. The notes of all features are a chunk of their
  own (`virtual:workshop-notes`), loaded the first time a pointer, a press or
  focus reaches a "Notes"; the page's bundle only knows which entries have
  some. For the owner only, "Decisions · N" sits on the same line: the
  decisions of `plan/decisions.json` whose `about` names the entry's area. That
  line never wraps and has its height from first paint whenever it can appear:
  until `me` answers, the `workshop-hint` cookie says whether "Decisions" may
  come, and a line given room keeps it.
- An open entry or suggestion shows its thread and a box to add a comment. The
  bot's history comments are history lines in the same thread, in time order:
  no avatar and no box of their own, one line each: what changed, the time and
  the change on GitHub. What changed is one phrase per change, joined by
  ` · `: "Text changed" (`name` or `what`, once), "Notes edited", "Needs
  changed", "Moved to another part" (`area`), "Suggestion changed", "Surfaces
  changed", "Taken", "Later", "Dropped", "Verdict cleared", "Built";
  `suggested_in` says nothing, and no raw key ever shows. A phrase too long for
  the line is cut with an ellipsis; the time and the link keep their place. "Text changed"
  opens the name and the sentence before and after: the page reads
  `plan/features/<id>.json` at the two commits from
  `raw.githubusercontent.com` only then, and shows "not available" when it
  cannot. While the thread loads, each comment and each history line the live
  state counts holds its place. A visitor who is not signed in gets "Sign in to comment" and, under it,
  "or comment directly on GitHub", the one link to that row's issue; a signed-in person
  gets the box and "GitHub #N". Until `me` answers, the panel keeps the room of
  the shape the `workshop-hint` cookie predicts, so it moves only when that
  cookie was stale.
- The strip under the map links to what is left out (entries without an area,
  and every dropped entry), to Latest and to the suggestions. Latest (`#latest`)
  lists the newest 30 comments and history lines across the entries, newest
  first, each opening its entry (a click with a modifier key, or a middle
  click, is left to the browser). While it loads, ten one-line placeholders hold
  its place and nothing sits below the list in its column.
- A person who can decide also gets "Decisions" (`plan/decisions.json`, newest
  first, carried in the page's bundle as the rest of the plan and shown to
  nobody else), the number of entries still to decide on each part, and a link
  to the first one in build order. After a verdict the next entry waiting in
  the same area opens, else the lowest one anywhere. Each open row has its own
  700 ms guard, and its verdict buttons look dimmed until it has passed; a row
  that becomes the current one after a verdict starts its guard again and is
  brought into view. Enter goes to the open row that holds the focus, else to
  the row opened last, and only when that row's verdict buttons are entirely in
  the visible part of the window. A tap during a view transition is ignored.
  The verdicts are take, later and drop; a verdict takes no note. Verdicts are
  for entries only, and never for a built one: the server refuses them.
  Nobody else is offered the decisions; their address shows the map.
- The header's account slot has a fixed width from first paint: signed in when
  the readable `workshop-hint` cookie is there (a long login is cut by an
  ellipsis), signed out otherwise. It changes only when that cookie was stale.
  Under "Sign in with GitHub", on hover and keyboard focus, a tooltip tied to
  the button says "or comment directly on GitHub", taking no room.

Addresses: `#<area>`, `#<area>/<entry id>`, `#suggested`, `#left` and
`#decisions`, each with an optional open row (`#suggested/issue-<n>`,
`#decisions/decided-<id>`), and `#latest`. An entry is found by its id whatever
area the address names, and the address is rewritten; an entry that no longer
exists opens its area. The older `#entry-<id>` and `#issue-<n>` open that row
where it now lives.
Back and Forward move between places; opening a row replaces the address.

| File | What |
| --- | --- |
| `App.tsx` | Places, addresses, focus, keyboard, the layout. |
| `useWorkshop.ts` | Live state and every write: the load, pending writes, the verdict guard. |
| `MapView.tsx`, `Tiles.tsx` | The map and the tiles. |
| `AreaView.tsx`, `Suggest.tsx`, `Latest.tsx`, `Decisions.tsx` | An opened area and what is not carried over, the suggestions, the latest changes, the decisions. |
| `Entry.tsx` | One row and what it shows open: notes, decisions, the thread with its history lines. |
| `routes.ts`, `plan.ts`, `map.ts`, `data.ts` | Addresses, the rows on top of the plan's model, the drawing, the build-time reader. |
| `api.ts`, `writes.ts`, `memory.ts`, `RichText.tsx`, `Markdown.tsx`, `Boundary.tsx` | The API client, the page's own writes (a list of values set, kept a day; its comments six minutes), the remembered answer and person, text from GitHub, notes, the fallback list. |

## Storage

GitHub issues of `amontlabs/silo` are the only storage. There is no database,
and nothing is written at run time apart from GitHub issues.

| Label | What the issue is |
| --- | --- |
| `workshop:entry` | One feature of `plan/features/`, created by the bot. The body ends with `<!-- silo-synced: <commit> -->` and `<!-- silo-entry: <id> -->`; the server removes both before answering. |
| `workshop:suggestion` | A feature suggested by a visitor. |

An issue that carries the retired label `workshop:decision` is not part of the
Workshop. The owner's verdict on an entry is one label: `verdict:take` or
`verdict:later`. A dropped entry has `verdict:drop` and is closed as "not
planned". A built entry is closed as "completed". Votes are 👍 reactions and
comments are issue comments. The history is the bot's history comments, whose
last line is `<!-- silo-change: {…} -->` (see `tools/plan/README.md`); the
server gives it to the page as `change`. Only a comment by the Workshop's own
App is read for it: its `performed_via_github_app` id is the configured App id
(`WORKSHOP_APP_ID`), or its author is the App's account, `<slug>[bot]` (the
slug read from the installation). A comment by any other bot is an ordinary
comment.

An entry being built now also carries `workshop:building` ("Being built now").
`plan start <id>` sets it, `plan stop <id>` removes it, and `plan sync`
removes it when it closes the entry once its file says `built` (see
`tools/plan/README.md`). A verdict
other than take (`later`, `drop`, or cleared) removes it in the same
write, so an entry taken again is not shown as being built. The server counts
the label only on an open entry that also carries `verdict:take`, so a removal
that failed, or a label left on an issue reopened by hand, shows nothing.

## The API

All endpoints are under `/api/workshop/` on the site's own origin and answer
JSON.

| Endpoint | Who | What |
| --- | --- | --- |
| `GET state`, `GET state?fresh=1` | anyone | Every issue with a Workshop label, open or closed: kind, entry id, title, state, verdict, 👍 count, comment count, author, dates. An open entry carrying both `workshop:building` and `verdict:take` also has `"building": true`; the field is absent on every other issue. An entry with history comments among those held has `updates`, their number. `latest`: the newest 30 comments held on entries, newest first, each `{ n, entry, author, bot, createdAt }` with `change` for a history comment, else `text`, its first line cut to 140 characters. `"old": true` when the snapshot it was answered from was over 30 seconds old; the field is absent otherwise. `fresh=1` (that exact value) waits for fresh numbers first, as described under the snapshot below. |
| `GET me` | signed in | `{ login, avatar, canDecide }`; `canDecide` is false when the permission could not be read (the next `me` tries again). 401, with the `workshop-hint` cookie cleared, only when there is no valid session. |
| `GET issue?n=` | anyone | Body text (first 20,000 characters), 👍 count from the issue's reaction summary, and the latest 100 comments held, oldest first, as plain text, a history comment with its parsed `change`; `more: true` when GitHub counts more. The same public answer for everyone. |
| `GET vote?n=` | signed in | `{ on }`: whether the person has given a 👍, read with their own token. |
| `GET login?return=` | anyone | Redirects to GitHub. `return` must be a path under `/workshop`. |
| `GET callback` | GitHub | Finishes sign-in and redirects to the return path. |
| `POST logout` | anyone | Revokes the person's whole grant for the App (all their sessions), then clears the cookies whatever happened. Answers `{ "ok": true, "revoked": true\|false }`. |
| `POST comment {n, text}` | signed in | Comments as that person. Answers `{ "ok": true, id, comments }`: `comments` is the comment count GitHub gave in the issue read made before posting, plus 1 when this request posted the comment (not when it was found already posted); absent when that read gave no count. |
| `POST vote {n, on}` | signed in | Adds or removes that person's 👍. Answers `{ "ok": true, up }`: `up` is the 👍 count from the issue's reaction summary in the read made before writing, plus 1 when GitHub created the reaction (a 👍 already there adds nothing), minus the person's 👍 removed, never below 0; absent when that read gave no count. Neither answer costs a GitHub call of its own. |
| `POST suggest {title, text}` | signed in | Opens an issue as that person and labels it `workshop:suggestion`. Returns `{ number }`. |
| `POST verdict {n, verdict}` | can decide | Sets (`take`, `later`, `drop`) or clears (`null`) the verdict of an entry; the `verdict` key is required and a `note` is refused. An entry closed as completed (built) is refused with 409 `built` before anything changes. Any verdict but `take` also removes `workshop:building` when the issue carries it (no call otherwise); if that removal fails, the verdict still stands. |

A comment that the same person posted on that issue in the last five minutes is
not posted again.

Errors are `{ "error": code, "message": plain words }` with 400 (bad input),
401 (not signed in, or the session is gone), 403 (not allowed, or a request
from another site), 404 (not a Workshop issue), 405, 409 (a verdict on a built
entry), 413 (body over 64 KB),
429 (GitHub's rate limit or one of ours), 502 (GitHub failed or took over 8
seconds) and 503 `not_configured` (missing or invalid App settings). A
suggestion whose issue was created but could not be labelled answers 502 with
`"error": "github"` and the issue's `number`. `login` and `callback` never
answer JSON: any failure redirects to `/workshop?signin=failed`; cancelling on
GitHub returns to the page.

Each comment is cut to 5,000 characters and then carries `"cut": true`. Every
marker is read with the readers of `tools/plan/model/` on the whole body or
comment, and the text is cut after: one definition, shared with `plan`. The
change marker is read only on the App's comments, so a person's comment never
reaches that reader.

An issue is a Workshop issue when it carries exactly one of the three kind
labels and is not a pull request.

Comments are limited to 5,000 characters, titles to 120
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
404. Neither read sends `Vary: Cookie` or reads a cookie. Their `Cache-Control`:

| Read | `Cache-Control` |
| --- | --- |
| `state` | `public, s-maxage=10, stale-while-revalidate=86400` |
| `state?fresh=1`, answer not old | `public, s-maxage=10` |
| `state?fresh=1`, answer still old | `private, no-store` |
| `issue` | `public, s-maxage=10, stale-while-revalidate=60` |

**What a read waits for.** `state` never waits for GitHub once the instance
has a snapshot: it answers at once from the snapshot it has, whatever its age,
with `"old": true` when that snapshot is over 30 seconds old. `old` is decided
when the instance answers: a copy the CDN serves later keeps the body it was
made with, so it can be hours old without `old`. The page therefore draws what
it gets and asks `state?fresh=1` once when the answer has `old: true`, or its
`Age` header is above 10, or its `fetchedAt` is more than 2 minutes before
now. `state?fresh=1` waits until the snapshot is at most 30 seconds old: it
joins the refresh that is running (or that will start within the wait), or
starts one, and waits for it up to 8 seconds; then it answers from the
snapshot it has, still with `old` when the refresh failed or did not end in
time. While GitHub has asked us to wait, it starts no refresh and answers at
once, with `old`. Only the exact value `fresh=1` does this; any other
value, and any other parameter, is ignored. `issue` waits the same way, up to
1.5 seconds, when its snapshot is over 30 seconds old. An instance with no
snapshot yet makes every read wait for its first refresh. Every wait ends at
the request's 12-second deadline at the latest.

**What a read costs.** Nothing: no `state` or `issue` request calls GitHub.
A read may start a refresh, which runs on its own ceiling.

**How soon a change is seen.** The person who wrote sees their change at once:
the page shows it itself. Others on the same instance see it within a second
or two, from the patch and then the refresh the write starts. Everyone else,
including a new suggestion on other instances, sees it within about 40
seconds: up to 10 seconds of CDN cache plus the 30-second refresh of the other
instances. A plain `state` may be older than that: from an instance that had
not refreshed for a while, and then it says `old`, or from a copy the CDN kept,
which the page tells by its `Age` header or its `fetchedAt`. In either case
the page's `state?fresh=1` that follows brings the numbers to at most 30
seconds plus the 10 seconds of CDN cache.

- **Refresh.** A read that finds the snapshot over 30 seconds old starts a
  refresh, unless one is running or waiting to start, and waits for it as
  described above (`state` not at all). Any request that changed something on GitHub (a comment, a vote, a
  suggestion, a verdict, even one that failed halfway) starts one
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
  (an instance that was frozen) does not set the 60-second wait. An instance with
  no snapshot yet makes its first requests wait for the refresh, at most until
  their 12-second deadline. A refresh builds a new snapshot and swaps it in
  only when it completed, so a refresh that fails halfway leaves the previous
  one intact.
- **Staying alive.** Work that outlives the response (refreshes and the label
  retries) is handed to Vercel's `waitUntil()`, which
  "extends the lifetime of the request handler for the lifetime of the given
  Promise"; such promises "have the same timeout as the function itself"
  ([@vercel/functions API reference](https://vercel.com/docs/functions/functions-api-reference/vercel-functions-package),
  read 2026-11-16). The server calls it as that package does (`wait-until.js`
  and `get-context.js` of 3.11.3, read 2026-11-17): through the request
  context Vercel keeps under `Symbol.for('@vercel/request-context')`. The
  package itself is not used: from 3.11 its entry point loads 153 files from
  21 packages. Correctness does not depend on it: the lost-refresh rule
  and the waits above cover a platform that stops the work.
- **Issues.** A refresh reads one list per Workshop label, entries and
  suggestions at the same time (`labels=workshop:…`, all states, newest first,
  100 a page, at most 20 pages for the two together, taken by whichever list
  asks first), so nothing else in the
  repository is fetched or counted against the cap, and issues without exactly
  one Workshop label or that are pull requests are skipped. The two lists are
  merged entries first, so an issue in both is read as an entry and entries
  come first against the 2,000 cap.
  `building` is read from the labels of the same lists, so it costs no call of
  its own, and a label added or removed on GitHub shows at the next refresh.
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
  them by id. The first comment page is asked for at the same time as the
  issue lists; GitHub's answer is reduced to Workshop comments as soon as the
  lists are read, and the next pages are asked for only then. When an issue
  list or the comment listing fails, the refresh's other calls are aborted,
  and the refresh fails with that first failure.
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
a visitor does without signing in can make `verdict`, `suggest`,
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

With each of the two lists on one page, a refresh is 3 calls (2 list pages,
which are `304` and free when unchanged, and 1 `since` comment call, which
always counts against GitHub's limit):

- **No writes:** one refresh every 30 seconds, 120 an hour: 120 × 3 = 360
  calls in our ceiling, of which about 120 count at GitHub, plus 6 full
  comment reloads whose unchanged pages are free: about 130 calls an hour.
- **Writes without pause:** write-started refreshes are at most one every 5
  seconds, 12 a minute: 12 × 3 = 36 calls a minute (2,160 an hour), under
  the refresh ceiling of 40. A refresh stopped by our own
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
  answer 429. The reads a whole action needs are checked before its first
  call.
- A session GitHub has rejected (a 401, an invalid refresh token, a revoked
  grant) is remembered by a hash of its token for a day (at most 10,000) and
  answered 401 without calling GitHub; a request without a valid session calls
  nothing.
- A sign-in cookie works for one callback per instance (its id is remembered
  for its 10-minute life, at most 10,000).
- History comments older than the comments the server holds (the latest 100
  per issue, at most 5,000 in all) are not seen as history: GitHub's comment
  count still includes them, so the page counts them as people's comments.
- Every GitHub call has an 8-second limit, and every handler has a 12-second
  deadline for all its GitHub calls and every wait together, including waits
  on shared work such as an installation token another request is fetching
  (the platform stops a function at 15 seconds); past it, the answer is 502.
  Steps are ordered so that a cut is safe to retry: `verdict` changes only
  labels and state, each step a no-op when already done; `comment` skips
  posting when the same person posted the identical text on that issue in the
  last five minutes.
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
| `website/server/issues.mjs` | Kinds, verdicts, the markers (read by the plan model), which comments are the App's own, and the cuts. |
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
| `website/scripts/workshop-refresh.test.mjs` | Refreshes started by writes, coalescing, the floor, the idle refresh, what each read waits for and its headers, the parallel refresh and its failures. |
| `website/scripts/workshop-abuse.test.mjs` | What a visitor or a replayed cookie can make the server do, memory, waits and retries. |

All of them run with the website's `npm test`.

The server uses Node's standard library (`crypto`, `fetch`, `http`) and no
package. `prepare-vercel.mjs` refuses a file the function would load from
`website/node_modules/`, and the whole function is 19 files, about 100 KB.

## Build and deploy

```sh
npm --prefix website run build          # the static site, in website/dist
npm --prefix website run build:vercel   # the site and the API, in .vercel/output
```

`npm --prefix website run build:vercel` builds the site and runs
`prepare-vercel.mjs`, which writes `.vercel/output/` at the repository root
following Vercel's
[Build Output API v3](https://vercel.com/docs/build-output-api/configuration):

- `static/`: the built site.
- `functions/api/workshop/state.func/`: one Node.js function
  (`.vc-config.json`: runtime `nodejs24.x`, handler
  `website/api/workshop.cjs`, launcher `Nodejs`). It holds the files it loads
  at their paths in the repository: `website/api/` and `website/server/` at
  any depth, and the plan's
  model, `tools/plan/model/*.js` with `tools/plan/package.json` (which makes it
  ESM). The script finds what loads by loading the entry point in a separate
  process with a resolve hook, and refuses by name, before writing anything,
  any other file it loads, in `website/` or outside it. The other
  endpoints (`me.func`, `issue.func`, `vote.func`, …) are symlinks to it, so
  Vercel creates one function served at every `/api/workshop/<endpoint>` path,
  as the [primitives reference](https://vercel.com/docs/build-output-api/primitives#functions)
  allows. An unknown endpoint is Vercel's 404.
- `config.json`: `version: 3` and its routes. The first adds
  `Content-Security-Policy: frame-ancestors 'none'` and `X-Frame-Options: DENY`
  to every path and continues. The second,
  `{ "src": "^/workshop/?$", "dest": "/workshop/index.html" }`, serves the
  Workshop page at both `/workshop` and `/workshop/`. `{ "handle": "filesystem" }`
  hands every other path to the filesystem. After `{ "handle": "hit" }`, so only
  for a file that exists, the last route adds
  `Cache-Control: public, max-age=31536000, immutable` to Vite's hashed files
  under `/assets/` and to the font files (`/fonts/*.woff2`): a browser keeps
  them and does not ask again, so a font is never edited in place and a new one
  gets a new name.

The script fails when the build has no Workshop page (`workshop/index.html`, or
`workshop.html`). `--out <dir>` writes the output elsewhere; the script refuses
any directory whose path does not end in `.vercel/output`, so it never deletes
anything outside such a directory. Both
`/.vercel` and `website/.vercel` are ignored by Git.

The Vercel `silo` project is connected to `amontlabs/silo`. Pushes and merges to
`main` deploy production; other branches get preview deployments without the
Workshop's secrets. The project uses Node.js 26 and the repository root as its
root directory, where Vercel reads `.vercel/output` after the build.
`vercel.json` at the repository root sets the build command,
`npm --prefix website run build:vercel` (`npm --prefix website run build`
produces only the static site in `website/dist`), and the install command,
`npm --prefix website ci`, so the project's own settings for them do not matter.

For a manual deployment from the repository root:

```sh
npm --prefix website test
npm --prefix website run build:vercel
npx --yes vercel@59.16.0 deploy --prebuilt --prod --yes
```

For a new checkout, first run `npx --yes vercel@59.16.0 link --yes --project silo`.
The prebuilt deployment uploads only `.vercel/output/`, which Git ignores along
with Vercel's project metadata and local environment files.

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
`amontlabs/silo` only ("Only select repositories"). Create the six labels in
the repository (`plan sync` does): `workshop:entry`, `workshop:suggestion`,
`verdict:take`, `verdict:later`, `verdict:drop`, `workshop:building`.

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
request; `verdict` works only on entries, and never on one closed as completed.
A verdict adds and removes only `verdict:*` labels, one at a time, and, for any
verdict but take, `workshop:building`, so other labels changed by someone else
are kept. It posts nothing. Clearing a verdict reopens only a dropped entry. A 401 on the installation token is a 502 and
drops the cached token; it never signs anyone out. *Limit:* if labelling a
suggestion fails twice, the new issue stays outside the Workshop until a
background retry labels it (in memory, on that instance only), and the answer
carries its number.

**Deciding.** `canDecide` is GitHub's answer: the person's permission on the
repository (`write`, `maintain` or `admin`), read with the installation token
and cached five minutes by GitHub account id, for yes and for no; that one
cache serves `me` and `verdict`. The permission is read for the
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

## Not verified yet

The Workshop went live on 2026-11-17. That first deployment showed that
Vercel's Git build uses `.vercel/output` at the repository root, that Vercel
follows the symlinked functions and loads the `.cjs` handler, and that the Plan
workflow's sync creates the labels and one issue per feature. Everything else
was run against the fake GitHub and in Node. These still depend on the real
services:

- the token exchange body format: the server sends JSON with
  `Accept: application/json`, including `code_verifier` and `repository_id`;
- whether a refreshed user token stays limited to the repository;
- a visitor removing their own 👍 with their user token;
- the collaborator permission endpoint with only the Metadata read permission;
- that `waitUntil()` of the request context keeps a Build Output
  Node.js function alive after the response, as documented for Vercel
  Functions (the code does not depend on it);
- that GitHub answers `304` to the conditional issue and comment list pages as
  documented, and that the reaction summary in the issue list moves the page's
  ETag when a vote changes.
- the verdict half of the Plan workflow (the owner's click becoming a commit
  on `main`), and what `tools/plan/README.md` lists as not verified.

## Why built this way

- **giscus** stores comments in GitHub Discussions and has no place for a
  verdict or a vote that the owner's page counts.
- **utterances** signs people in with an OAuth App that needs the broad
  `public_repo` scope, write access to every public repository of the person.
  A GitHub App's user token is limited to the App's permissions and, here, to
  one repository.
- **A database of our own** is something to run, secure and back up. Issues
  keep the history, the authorship and the moderation tools on GitHub.
- **Verdicts as labels, then commits.** A click sets a label; a workflow then
  records it in the feature's file with its own token (`plan pull`), so the
  App's key, held by Vercel, never gets write access to the code. See
  `tools/plan/README.md`.
