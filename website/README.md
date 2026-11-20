# Silo website

The public site at [silo.amontlabs.com](https://silo.amontlabs.com), built with
Vite. It has two pages:

- **Home** (`index.html`, `src/style.css`): the headline and the pitch,
  centred in the height the window leaves, then the way into the Workshop with
  one line on where Silo stands, and under it the "in heavy development" notice. Its texts are interim, until the owner settles
  the new design. It has no script of its own and nothing on it is read from
  the plan. The page shows once its fonts are in (at most 1.2 s), so nothing
  changes width: it preloads every face it waits for, and the deployed site
  lets a browser keep the font files, so a later visit does not wait.
  A link to the site shows `public/og.png` when shared: a 1200 × 630 capture
  of `scripts/share-image.html`, opened from the dev server at
  `/scripts/share-image.html`.
- **Workshop** (`workshop/index.html`, `src/workshop/`), at
  [silo.amontlabs.com/workshop](https://silo.amontlabs.com/workshop): Silo's
  plan, the parts of the product as tiles, each with its decisions and
  features, the decisions of the whole product, and the features people
  suggest. Anyone signed in with GitHub can suggest a feature, and vote and
  comment on a suggestion, a decision or a feature. Its API lives under `/api/workshop/` (`server/`,
  `api/`). While the plan holds no part, it is the same page with no tile but
  "Suggest a feature".

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
`http://127.0.0.1:4173/workshop/?fixture=today`; add `&as=in` to
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
an issue for the first three entries of `plan/` (or of the plan `WORKSHOP_PLAN` names), so that
the other rows have no thread yet and can be tried; the first comment of the first issue has two 👍.
It also holds two issues with the retired label
`workshop:decision` (outside the Workshop), a suggestion, comments, 👍 votes,
an ordinary issue and a pull request.
Sign-in goes through the fake's page, which offers two people: `owner-fake`
and `visitor-fake`. Everything lives in memory and resets on restart.

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
2560 × 1440, 1024 × 1366 and 390 × 844, on the plain view, the first part
that has rows, `#product` when the whole product has rows, `#latest`
and `#suggested`. The parts and the whole product's
rows are read from the page's data
(`window.__workshopData`, set by `main.tsx`), and the places to visit are chosen
from its counts of rows (`placesOf`, tested in `load-check.test.mjs`); whether
it draws a map is read
from the page. For each load it prints when the page first
painted, when the account, the votes and "Suggest a
feature" appeared, what the page's first frame held, and every layout shift
with their total. Every one of these loads is a first visit: its first frame
must hold the sign-in button (unless `--signed-in`), "Suggest a feature" on the
plain view, and on the first place with rows (the first part that has rows,
else the whole product) a vote placeholder on every row. Then, in one browser profile at 1440 × 900, that
place (the plain view when the plan has no row) is visited twice (the second
visit must have the votes in its first frame), and a person signs
in through the fake GitHub's page as `owner-fake` (so the session cookie, its
marker and the remembered person are real) and visits again: the login must
be in its first frame (the app's first frame: the markup the build drew does not count). Then
an entry of that place without comments (one with no thread yet included) is opened before the answers, at 1440 × 900 and
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
`--signed-in` fakes the `me` answer as a signed-in person. Last, with the page's script
held (run it with `--slow-script 3000`) and the API held longer, it loads the
built page at 1440 × 900: on the plain view the first painted frame must hold
the map (or the tiles, without a map) when the plan has parts, and the sign-in
button, the footer rule
must hold, and the title row, the map or the tiles, the footer and the account
slot must not move and no layout shift happen when the script takes over; on
the first place with rows the plain view must never be painted and the place
must be in the first frame after the takeover; and on a
second visit, the state remembered, nothing under the title row may move by
half a pixel or more when the script takes over, at 1440 × 900 and 390 × 844.
A plan with no part is checked at the places it has, and passes.
`npm test` does not run it.

To build and check the page on another plan than `plan/` (the real plan may be
in the middle of a change), name its folder in `WORKSHOP_PLAN` for the build
and for the runner:

```sh
export WORKSHOP_PLAN=$PWD/tools/plan/test/fixtures/sample
npm --prefix website run build
npm --prefix website run workshop:dev -- --fake
```

## Test

```sh
npm --prefix website run typecheck
npm --prefix website test
```

`npm test` runs the Node tests in `scripts/*.test.mjs` (the theme script, and
the Workshop server against its fake GitHub, including the Vercel output
layout and the local server), then the page's Vitest tests.

The fake GitHub answers like GitHub where it matters to the server: expiring user tokens
with PKCE, a single-use refresh token, grant revocation, ETags and `304` on
listings, the repository-wide comment list with `sort`, `direction` and
`since`, comment edits and deletions, primary and
secondary rate limits, locked issues, and a hook that delays or fails any call.

## The Workshop page

React and TypeScript. At build time the `workshopData` plugin of `website/vite.config.ts`
reads `plan/` (or the folder `WORKSHOP_PLAN` names) with `tools/plan/read` and
checks it with `tools/plan/check` (`data.ts`); any error of `plan check` stops
the build with the file, the field and the fix. The same file calls `readSteps`
and `entryHistory` of `tools/plan/history` on the repository, once per build:
that is the history of each entry (see "History" below). A part with no feature is fine.
A part whose id is `product`, `left`, `suggested` or `latest` stops the build:
those are the page's own addresses, and the message names the part and the fix.
`map.ts` holds the drawing (`SILO_MAP`): where each part sits on the full map
and on the folded one, the zones, the text that is not a part, and the wires.
`SILO_MAP` is empty: with no place in it there is no map, any plan passes, and
the page shows the parts as tiles only, without the Map and Order control. Once
the drawing places one part, it must place every part of `plan/parts.json` and
nothing else, or the build stops. `map-example.ts` is a made-up layout of five
parts that shows every kind of thing a layout can hold; the tests draw their
fixture plan (`fixtures.ts`) with it, and the page never shows it.

The page has no way to take or drop an entry and the server has no verdict route:
decisions and features are taken or dropped with `plan take` and `plan drop`.

`npm run build` then runs `scripts/prerender.mjs`, which renders the plain
address `/workshop` (`src/workshop/prerender.tsx`: nothing remembered, nobody
signed in, the load pending) and writes it into `dist/workshop/index.html`: the
title row, the map when there is one, the tiles and the row under them in
`#workshop-root`, "Sign in with GitHub" in the header's account slot (with its
tooltip), and, in the page's `<noscript>`, the line
that says what needs JavaScript followed by every part with its decisions and
features and then the decisions of the whole product, as a plain list (id,
name, state, and "Replaced by" for a replaced decision; `NoScript.tsx`). The root and the slot carry `data-prerendered`
until the script takes over. A
prerender that throws fails the build; the dev server does not prerender.
Rendering at build time reads nothing of `window`, `document`, `location`,
`localStorage` or `matchMedia` (effects may). So the page is drawn whole before
its script arrives:

- The script takes over in one synchronous step (`takeover.ts`): React renders
  over the markup and commits before the next frame, and the drawn sign-in
  leaves the slot in the same task. On a first visit without a session at the
  plain address the first render is the same markup as the build makes it
  (`prerender.test.tsx` compares them, on the fixture plan, an empty plan and
  the plan tool's sample); with something remembered it differs on purpose.
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
  "Sign in with GitHub" replaces the placeholder, marker or not. The plain list
  (`Boundary.tsx`) gives each part's decisions and features with their state,
  then the decisions of the whole product.
- Without JavaScript the tiles show and, under them, the line that says what
  needs JavaScript and the plain list of every part's rows and the whole
  product's. The list is inside `<noscript>`, so a browser that runs the script
  never draws it and the first paint with JavaScript is unchanged.
- The page preloads every face it waits for and shows as soon as they are in
  (at most 1.2 s), without waiting for its own script: the check runs right
  after the fonts' stylesheet.

An entry's status comes from its file alone, through the model's `entryStatus`
(`tools/plan/model/status.js`); `plan.ts` says it in the page's words. Nothing
on GitHub changes it.

What a person sees:

- The parts as tiles in the order of `parts.json`; parts that hold an entry of
  the first five stages sit in the first band, the others after "After that".
  With a map, at 1200 px and wider, the map of the parts (Map) or the tiles
  (Order); narrower, the tiles only. Opening a tile is a view transition where
  the browser has them.
- A part's rows are its decisions and features, in the order of `order.json`,
  drawn by the same row. A feature is planned (taken, ready or not), built (its
  file says `built`), dropped, or not decided. A decision's name is its `asks`
  and its body its `answer`; it is decided (taken), not decided (open or
  answered), replaced or dropped. A decision is replaced when its file has a
  `replaced_by`, which `plan take` writes into each decision a taken decision
  names in `replaces`; the page passes it to the model, which reads the state
  from that key. A replaced decision is not counted as decided: a part's
  decisions line shows "N replaced" before "N dropped". It stays at its place
  in the order of `order.json`, looks as a dropped row does, and is not in
  "Left out". Opened, it shows what it showed when taken and the line
  "Replaced by", with the question of the decision that replaced it as a link
  that opens it where it lives. A decision that names others in `replaces`
  shows "Replaces" (taken) or "Would replace" (no verdict) with their questions
  as links; a dropped one shows no such line. In Latest, a change of
  `replaces` reads "Replaces changed".
- A part's state, counts and the line under the title come from its features
  only: done (something built, nothing planned), next (the part holding the
  first planned feature in order), planned, not decided yet. An open part shows
  one dot per feature, its features' counts, and its decisions' counts on a
  line of their own ("Decisions: 1 decided · 2 not decided").
- Opening a part folds the map to a strip that stays on screen (when there is
  a map and the window is at least 760 px tall); the part's rows open in place,
  each on its own. Opening or closing a row never moves it or the rows above it.
  The row opened last is the one in the address. A row with a thread loads its
  comments when a pointer, a press or focus reaches it, and on every opening (a
  row with no thread yet loads nothing);
  it shows itself open at once, its panel grows when the comments are in or
  after 180 ms, and while they still load each one the count announces holds
  its place. An entry named by the address
  waits closed until the first answer when no live state is shown yet. An
  address naming something the state shown lacks falls back to the plan only
  once the load has answered or failed, and the `fresh=1` ask has too when one
  was made. The person's own vote on an opened row and their votes on its comments
  come from one `GET vote`, asked only once `me` has answered on this visit, so a session found gone
  on page load says nothing; a row with no thread yet asks nothing. The suggest form shows whenever the person is shown signed in and
  keeps its draft; it sends only while a state is shown.
- What the page shows first: the plan is in the bundle. The live state (votes,
  comments, suggestions) and who is signed in come from
  `GET state` and `GET me`, and the page remembers both in local storage
  (`memory.ts`): the newest answer as the server sent it, and the signed-in
  login and avatar. The page's own confirmed writes are
  kept apart (`writes.ts`), as a list with each one's time, of values set: a
  new suggestion, and for a vote or a comment the count the server
  answered for that write (`up`, `comments`; the count shown plus or minus one
  when an older server answers none). A write that made or found an entry's thread
  also says so (`opens`), which adds the thread to an answer that lacks it and never
  replaces one it has. Never a change to a count: the server
  patches its snapshot in place, so an answer may already count a write made
  after its `fetchedAt`. What the page shows is the answer with every write applied in
  the order it was sent, except those the answer was fetched more than a minute after; a write
  is dropped after a day, the page's own comments and the count the server answered for the
  person's vote on a comment after six minutes. Both are drawn in the
  first frame. The person is used only while the `workshop-hint` cookie is
  there, and forgotten on sign-out, on a 401 and when `me` answers signed out; a
  state over 7 days old or of another shape is ignored (a state with no `threads`, as an
  older page stored it, is another shape), and so is a field that
  an older page stored and this one does not read. Without the cookie the
  visitor is signed out from the first frame ("Sign in with GitHub", "Sign in to
  comment", "Sign in to suggest").
- The load: `state` is asked once. Its answer is shown unless the state shown
  is newer (by `fetchedAt`). An old answer (`old: true`, an `Age` header over
  10, or fetched over 2 minutes ago) is followed by one `state?fresh=1`, shown
  in place; a failure of that ask says nothing. A failed first ask is tried
  once more after 1.5 s (not on `not_configured`); then "Votes and comments are
  offline." shows with "Try again", and remembered numbers stay on screen.
- While no state is shown and the first answer is to come, placeholders hold the
  room of what is known to come: the vote of every row, the
  header's account when the cookie is there and nobody is remembered, Latest's
  lines, in an open entry's panel "No comments yet." and the comment box (the shape most entries have; a thread with comments makes the panel grow once, as a slow one does), two rows of suggestions
  and the suggest form; notes have their own while their chunk loads. They
  pulse slowly (not with reduced motion), are `aria-hidden`, and their region
  is `aria-busy`. Nothing has a placeholder that may not come (comment counts,
  "N suggested", the link to an issue). What arrives after the first frames (numbers, the account, a new
  item of the key) fades in over 150 ms where it stands;
  nothing fades on navigation or when a row opens, or with reduced motion.
- An open entry shows, in this order, each part only when it has something: its
  text (a decision's answer, a feature's `what`); on an entry with no verdict,
  "Recommended: take" or "Recommended: drop" and why (`suggest`), then "A yes
  would add" (each item of `sets` whose id is not in force: its kind as a word,
  then its text; a word and its meaning, a principle and the question it asks,
  a property), "A yes would change" (kind and id, then "Now" the text in force
  and "After" the new one) and "A yes would remove" (each id of `removes` with
  its kind and the text in force), then "Would replace"; on a taken entry,
  "Added" (each statement of `adds` with its kind and text from the lists) and
  "Replaces"; "Needs" (each entry of `needs` by its question or name, as a link
  that opens it where it lives, with its state word; one not decided says "not
  decided yet"); on a feature, "Must keep" (the text of each property of
  `keeps`) and "Where it shows" (`surfaces`: "App", "Command"); then the state
  and "Replaced by". A part is a small quiet label with its lines under it, the
  texts at the size of the answer, as text and never as HTML. The id of the
  entry (not of a suggestion) shows next to the state in the monospace face
  with a "Copy id" button: a click writes it to the clipboard and the button
  says "Copied" for two seconds, in the same width; when the clipboard refuses,
  the id is selected for a hand copy and the button does not change. The change
  is announced politely. What the parts say of the statements (suggest, sets,
  removes, adds, keeps, surfaces, and every statement in force) is a chunk of
  its own (`virtual:workshop-facts`, `Facts`), loaded the first time a pointer,
  a press or focus reaches a row that has something in it (`Row.more`, known
  from the page's data) or such a row opens, with three placeholder lines in
  the place of its parts meanwhile; if it cannot load, the parts are left out.
  Then "Notes" (its `<id>.md`) and "History", side by side, each closed by
  default and opened in place. Notes: headings, paragraphs, lists, http and https
  links (an address may hold one level of brackets), inline code and code
  blocks become elements (`Markdown.tsx`); nothing is ever inserted as HTML,
  and anything else stays text. The notes of all entries are a chunk of their
  own (`virtual:workshop-notes`), loaded the first time a pointer, a press or
  focus reaches a "Notes"; the page's bundle only knows which entries have
  some.
- "History" is on every entry that has at least one change, whether or not it has a
  thread. Closed, its line reads "History", the last change's phrases and
  its date, and "N changes" when there are several ("History · Decided, 20
  Nov · 3 changes"). Opened, it lists the changes in place, newest first, one
  line each: the date, the phrases of that change joined by ` · `, and a
  "commit" link to that commit on GitHub. The phrases come from the names the
  plan tool's history gives (`tools/plan/history`, `plan.ts` `changePhrase`):
  "Added", "Moved to another part", "Text changed", "What a yes would do
  changed", "Needs changed", "What it must keep changed", "Where it shows
  changed", "Recommendation changed", "Notes edited", "Decided" on a decision
  and "Planned" on a feature, "Dropped", "Back to not decided", "Replaced by"
  (followed by the question of the decision that replaced it, a link that
  opens it) and "Built"; no name of a key ever shows. "Text changed" opens
  the title and the body before and after, whichever differ.
  The history is read from git when the site is built (`readSteps` and
  `entryHistory`, called by `data.ts`): nothing asks GitHub for it. A
  shallow clone is fetched whole once, with no credential and a five-minute
  limit per fetch: `git fetch --unshallow origin`, and when that fails (Vercel's
  clone has no remote) the commit being built from the repository's public
  address (`repoAddress`, the one the "commit" links use), so a preview branch
  gets its whole history too. When both fail, the build goes on and every list
  ends with "Earlier changes are not available in this build."
  (`BuildData.shallow`).
  A plan read from another folder (`WORKSHOP_PLAN`) has no history.
  The page's data holds, for each entry, only its last change and the number of
  changes (`Entry.history`), enough for the closed line. The changes with their
  texts are a chunk of their own (`virtual:workshop-history`, `Histories`),
  loaded the first time a pointer, a press or focus reaches a History, or Latest
  opens, with one placeholder line per change meanwhile; if it cannot load,
  the list says so and the next time tries again.
- An open row, a decision, a feature or a suggestion, shows its thread and a box
  to add a comment. A comment of the Workshop's App is an ordinary
  comment. While the thread loads, each comment the live state counts holds
  its place. A visitor who is not signed in gets "Sign in to comment" and, under it,
  "or comment directly on GitHub", the one link to that row's issue; a signed-in person
  gets the box and "GitHub #N". Until `me` answers, the panel keeps the room of
  the shape the `workshop-hint` cookie predicts, so it moves only when that
  cookie was stale.
- An entry's thread is the issue that `threads` of `GET state` gives for its id; the page
  matches none by itself, so an issue that merely names the entry is not its thread. A feature
  whose file has `suggested_in` shows once, as the feature, and its thread is that
  suggestion's issue, which is not listed among the suggestions.
- A row whose entry has no thread yet shows the vote with the thumb alone. Opened, it shows
  "No comments yet." and then the comment box (signed in) or "Sign in to comment", and no link to
  GitHub, since there is no issue yet; the room of that link stays empty, so the row taking its
  thread moves nothing in the panel. A row whose thread has no comment shows the same line.
- A comment or a 👍 on a decision or a feature is sent with `entry`, the id of the entry, whether or not the
  page knows its thread. The server finds the thread, or opens it at the first one.
- A comment or a 👍 on a suggestion is sent with `n`, its issue.
- The `n` of the answer becomes the row's thread at once, and the comment or the count shows without a reload.
- A row that takes a thread, or whose thread's number changes, while it is open asks for that thread and for the
  person's votes, as an opening does.
- The person's own comment stays shown from the page's kept writes until an answer holds it.
- A 👍 works the same on a decision, a feature and a suggestion, whether the entry is decided, planned, dropped
  or replaced.
- A comment that the server answers twice (a retry after an error) shows once.
- The page applies and keeps its own writes in the order it sent them, not the order their answers came.
- A thread closed on GitHub shows its comments, no box, and "This thread is closed on GitHub." with the
  link to the issue.
- A thread is closed when `state` says `closed`.
- A thread is also closed when the server answers a write 403 `closed`, which it does only to a write that names an entry.
- That answer carries `n`, which becomes the row's thread when the row had none.
- Under each comment of a thread is its 👍 button (`up` of `GET issue`), the same button as a row's: the thumb and, from 1 vote up,
  the count. Signed out, the button asks to sign in, as the row's vote does. Signed in, the button is
  pressed for the comments `GET vote` names. A click changes the count at once and sends
  `POST comment-vote`.
- The count the server answers for a vote on a comment is applied to a `GET issue` answer unless that answer
  was fetched more than a minute after the vote (`writes.ts`).
- A refusal takes back the page's own change, one up or one down from the count shown by then, and says
  "Vote: not saved." with the server's words.
- The strip under the tiles links to what is left out (every dropped decision
  and feature of the parts), to "The whole product" (`#product`: the decisions
  of `plan/decisions/`, with their own counts), to Latest, and to the suggestions. Latest (`#latest`) lists the newest 30
  comments and changes (from the history chunk) across the entries, merged by
  time, newest first, each opening its entry (a click with a modifier key, or a middle click, is left to the
  browser). While it loads, ten one-line placeholders hold its place and
  nothing sits below the list in its column.
- The header's account slot has a fixed width from first paint: signed in when
  the readable `workshop-hint` cookie is there (a long login is cut by an
  ellipsis), signed out otherwise. It changes only when that cookie was stale.
  The width the login and "Sign out" take is stored for the next visit, where
  the head script applies it before first paint; it is never applied during a
  visit, so on the first visit after signing in the login sits right-aligned in
  the reserved width.
  Under "Sign in with GitHub", on hover and keyboard
  focus, a tooltip tied to the button says "or comment directly on GitHub",
  taking no room.

Addresses: `#<part>`, `#<part>/<entry id>`, `#product`, `#suggested` and
`#left`, each with an optional open row (`#suggested/issue-<n>`), and `#latest`.
A row named by an address is looked for first
among the plan's entries, by id, whatever place the address names (an entry
whose id begins with `issue-` included), and only then among the suggestions;
the address is then rewritten to where the row is. An entry that no longer
exists opens its part. `#suggested/issue-<n>` for a suggestion that became a feature opens the feature, where it lives. The older `#entry-<id>` and `#issue-<n>` open that row
where it now lives. The old list of decisions and its address `#decisions` are
gone. No part can take the id of a place: the build refuses it (above).
Back and Forward move between places; opening a row replaces the address.

| File | What |
| --- | --- |
| `App.tsx` | Places, addresses, focus, keyboard, the layout. |
| `useWorkshop.ts` | Live state and every write: the load, pending writes. |
| `MapView.tsx`, `Tiles.tsx` | The map and the tiles. |
| `PartView.tsx`, `Suggest.tsx`, `Latest.tsx` | An opened part, the whole product's decisions and what is left out; the suggestions; the latest changes. |
| `Entry.tsx` | One row and what it shows open: the text, what the entry says of the plan's lists and what it needs, its id and the button that copies it, the state, notes, history, and the thread with the votes on its comments. |
| `routes.ts`, `plan.ts`, `map.ts`, `data.ts` | Addresses; the rows, statuses, parts' counts and states on top of the plan's model; the drawing; the build-time reader. |
| `api.ts`, `writes.ts`, `memory.ts`, `RichText.tsx`, `Markdown.tsx`, `Boundary.tsx` | The API client, the page's own writes (a list of values set, kept a day; its comments six minutes), the remembered answer and person, text from GitHub, notes, the fallback list. |
| `prerender.tsx`, `NoScript.tsx`, `takeover.ts` | The markup the build writes into the page, the list for a reader without JavaScript, and the script's takeover. |

## Storage

GitHub issues of `amontlabs/silo` are the only storage. There is no database,
and nothing is written at run time apart from GitHub issues.

| Label | What the issue is |
| --- | --- |
| `workshop:entry` | The thread of one entry of the plan, a decision or a feature. The bot opens it. Its body ends with `<!-- silo-entry: <id> -->`; the server removes that line before answering. |
| `workshop:suggestion` | A feature suggested by a visitor. |

Votes are 👍 reactions on an issue. Comments are issue comments, and a vote on
a comment is a 👍 reaction on that comment.

**An entry's thread** is the issue that holds the comments and votes on one
entry of the plan. An entry gets one only when someone writes on it: the first
comment or 👍 on the feature `moving-main` opens an issue titled "Moving main",
labelled `workshop:entry`, with this body:

```
Feature of the Silo plan: https://silo.amontlabs.com/workshop#computers/moving-main

<!-- silo-entry: moving-main -->
```

A decision's issue says "Decision of the Silo plan" and takes the decision's
question as its title. A title longer than 120 characters is cut at a word and
ends with "…". The link names the entry's part, or `product` for an entry of
the whole product. The server finds an entry's thread in this order:

1. The entry's file has `suggested_in: N`, and issue N is a Workshop
   suggestion: issue N is the thread. A feature written from a suggestion so
   keeps the suggestion's conversation.
2. An issue labelled `workshop:entry` ends with the entry's marker: that issue
   is the thread. When several issues name the same entry, the lowest number
   is the thread and the server ignores the others. `workshop doctor` reports
   them.
3. Otherwise the entry has no thread yet.

An entry's thread that is closed or locked on GitHub stays its thread.

**The plan's entries.** The server knows the plan from `plan-entries.json`,
which `prepare-vercel.mjs` makes from `plan/` at build time and ships beside
the server. For each entry it holds the id, the kind, the title (a decision's
`asks`, a feature's `name`), the part (null for the whole product) and
`suggestedIn`. The server never takes an id or a title from the page: a write
names an entry by its id, and the server looks the id up in this file. The
local server makes the same list from the plan when it starts. A deployed
function without the file logs it and answers 404 to any write that names an
entry.

An issue that carries the retired label `workshop:decision` is not part of the
Workshop. An entry's status comes from its file alone. A suggestion's status
comes from its issue alone: open is not decided, closed as "not planned" is
dropped, closed as "completed" is built. GitHub holds only what people add
(comments, suggestions and votes) and the threads the bot opens for them. The
history of an entry is read from git when the site is built. A comment is
marked `bot` when it is by the Workshop's own App: its
`performed_via_github_app` id is the configured App id (`WORKSHOP_APP_ID`), or
its author is the App's account, `<slug>[bot]` (the slug read from the
installation). It is otherwise an ordinary comment.

## The API

All endpoints are under `/api/workshop/` on the site's own origin and answer
JSON.

| Endpoint | Who | What |
| --- | --- | --- |
| `GET state`, `GET state?fresh=1` | anyone | Every issue with a Workshop label, open or closed: kind, entry id, title, state, 👍 count, comment count, author, dates. `reason` is `not_planned` for a closed issue that was closed as not planned (a dropped suggestion), `completed` for one closed as completed (a built suggestion), and null otherwise. `threads`: the issue number of each entry's thread, by entry id, for the entries of the plan that have one. `latest`: the newest 30 comments held on entries' threads, newest first, each `{ n, entry, author, bot, createdAt, text }`, `text` its first line cut to 140 characters. `"old": true` when the snapshot it was answered from was over 30 seconds old; the field is absent otherwise. `fresh=1` (that exact value) waits for fresh numbers first, as described under the snapshot below. |
| `GET me` | signed in | `{ login, avatar }`, read from the session cookie alone: it calls nothing on GitHub. 401, with the `workshop-hint` cookie cleared, only when there is no valid session. |
| `GET issue?n=` | anyone | Body text (first 20,000 characters), 👍 count from the issue's reaction summary, and the latest 100 comments held, oldest first, as plain text; `more: true` when GitHub counts more. Each comment has `up`, its 👍 count, from the reaction summary GitHub returns with the comment. The same public answer for everyone. |
| `GET vote?n=` | signed in | `{ on, comments }`. `on` says whether the person has given the issue a 👍. `comments` lists the ids of the issue's latest 100 comments the person has given a 👍. Both are read with the person's own token: the 👍 on comments in one GraphQL query. Only the newest 100 comments are looked at: they are the comments `GET issue` gives and the page shows. |
| `GET login?return=` | anyone | Redirects to GitHub. `return` must be a path under `/workshop`. |
| `GET callback` | GitHub | Finishes sign-in and redirects to the return path. |
| `POST logout` | anyone | Revokes the person's whole grant for the App (all their sessions), then clears the cookies whatever happened. Answers `{ "ok": true, "revoked": true\|false }`. |
| `POST comment {n, text}`, `POST comment {entry, text}` | signed in | Comments as that person. Answers `{ "ok": true, id, comments }`: `comments` is the comment count GitHub gave in the issue read made before posting, plus 1 when this request posted the comment (not when it was found already posted); absent when that read gave no count. With `entry`, see "Writing to an entry" below. |
| `POST vote {n, on}`, `POST vote {entry, on}` | signed in | Adds or removes that person's 👍. Answers `{ "ok": true, up }`: `up` is the 👍 count from the issue's reaction summary in the read made before writing, plus 1 when GitHub created the reaction (a 👍 already there adds nothing), minus the person's 👍 removed, never below 0; absent when that read gave no count. Neither answer costs a GitHub call of its own. With `entry`, see "Writing to an entry" below. |
| `POST comment-vote {n, comment, on}` | signed in | Adds or removes that person's 👍 on comment `comment` of issue `n`, with their token. `n` must be a Workshop issue and `comment` one of its comments, else 404. Answers `{ "ok": true, up }`, `up` counted as `POST vote` counts it, from the comment read before the write. |
| `POST suggest {title, text}` | signed in | Opens an issue as that person and labels it `workshop:suggestion`. Returns `{ number }`. |

A comment that the same person posted on that issue in the last five minutes is
not posted again.

### Writing to an entry

`POST comment` and `POST vote` take either `n`, an issue number, or `entry`,
an id of the plan, never both. With `entry`:

- An id that is no entry of the plan answers 404, before any call to GitHub.
- When the entry has a thread, the write goes on it as it would with that
  thread's `n`, and the answer carries `n`.
- Before a write to an entry's thread, the read of the issue the write makes
  anyway confirms that the issue still is the thread: it carries the label
  `workshop:entry` and its marker names the entry, or it is the suggestion
  that `suggested_in` names. An issue whose label or marker was changed on
  GitHub is no thread. The server then looks the thread up on GitHub once
  more, as before an opening, and goes on with what it finds, or opens one.
  An issue that was labelled `workshop:entry` or given a marker by hand on
  GitHub is found once the snapshot holds it, at the next refresh.
- When the entry has no thread, the server first reads the entry issues on
  GitHub with the App's token: the newest pages, until they reach the issues
  the snapshot already held a minute before its refresh began. The snapshot
  gives the older ones. A thread another instance has just opened is found
  this way. Without a snapshot, it reads up to 20 pages (2,000 entry
  issues).
- When that read finds none, the App opens the issue described under
  "Storage", labelled `workshop:entry`. What bounds
  the openings is described under "The installation token and its
  ceilings".
- When GitHub created the issue without the label, the App creates the label
  in the repository (one that exists already is fine) and adds it to the
  issue. When the issue still lacks it, the App closes the issue, and the
  answer is 502 with `"error": "github"`. No open issue without the label is
  left behind.
- Once GitHub has answered the opening and the issue carries its label, that
  issue is the entry's thread for this instance. The server puts it in the
  snapshot at once, before any read. GitHub's list of issues by label does not
  show a new issue at once: on 2026-11-20 it showed one about 30 seconds after
  it was opened, and nothing says how long it can take.
- After the opening, the App reads the entry issues again, to find a lower
  issue that names the entry. There is one when another instance opened one
  at the same moment. The lower one is then the thread, and the person's
  write goes on it. The App closes the issue it opened, as not planned, takes
  the label `workshop:entry` off it, and takes it out of the snapshot.
- When that read shows no lower issue, the issue just opened is the thread,
  whether the read shows it or not. When the read fails, or has no room left
  in the ceiling below, the issue just opened is the thread too.
- The person's comment or 👍 then goes on the thread with the person's own
  token. The answer carries `n`.
- Two requests for the same entry on the same instance share one opening. A
  request that comes after it finds the thread in the snapshot.
- A refresh of the snapshot keeps a thread this instance opened until a
  refresh's own list shows its number, and at most 10 minutes after the
  opening. Until then, every write on that entry from this instance goes on
  it, and no second issue is opened.
- An issue whose label `workshop:entry` was removed can stay in GitHub's list
  by label for some time, without the label in its own `labels`. Such an
  issue is no thread and no Workshop issue.
- When the issue was opened and the person's write then fails, the issue
  stays. The answer is the error, and it carries `n`. The next try finds the
  issue.
- Once the server knows the thread of the entry a write names, every answer
  to that write carries `n`, errors included.
- `POST vote {entry, on: false}` on an entry with no thread answers
  `{ "ok": true, "up": 0 }` and opens nothing.
- When GitHub refuses a write on an entry's thread with a 403, the server
  reads the issue once more. When it is closed or locked, the answer is 403
  `closed`: "This thread is closed on GitHub." No second issue is opened.

What can still happen: two instances that each open an issue for the same
entry before GitHub's list shows the other's each keep their own issue, and
each makes its person's write on it. Once the list shows both, the lowest
number is the thread. The write on the other issue stays on GitHub, and the
Workshop does not show it. `workshop doctor` reports the entry.

What a write costs on GitHub:

| Write | Calls with the App's token | Calls with the person's token |
| --- | --- | --- |
| The first comment on an entry | 3: the read of entry issues, the opening, the read again | 1: the comment |
| The first 👍 on an entry | 3, the same | 1: the 👍 |
| A comment on an entry whose thread the snapshot holds | 0 | 3: the issue, the person's recent comments on it, the comment |
| A 👍 on an entry whose thread the snapshot holds | 0 | 2: the issue, the 👍 |

Each read of entry issues is one page when the snapshot is fresh, and one
more page for each 100 entry issues opened since a minute before the
snapshot's refresh began. An entry written from a
suggestion costs one more read with the person's token: the suggestion's
issue. The App's token is never used to write a person's words, and a
person's token is never used to open an issue for an entry.

Errors are `{ "error": code, "message": plain words }` with 400 (bad input),
401 (not signed in, or the session is gone), 403 (not allowed, `closed` for
an entry's thread closed on GitHub, or a request from another site), 404 (not
a Workshop issue, not an entry of the plan, or not a comment of that issue),
405, 413 (body over 64 KB), 429 (GitHub's rate limit or one of ours), 502
(GitHub failed or took over 8 seconds) and 503 `not_configured` (missing or
invalid App settings). A write naming an entry whose thread the server knows
carries the thread's `n` in its error too. A suggestion whose issue was
created but could not be labelled answers 502 with `"error": "github"` and
the issue's `number`. `login` and `callback` never answer JSON: any failure redirects
to `/workshop?signin=failed`; cancelling on GitHub returns to the page.

Each comment is cut to 5,000 characters and then carries `"cut": true`. Every
marker is read with the readers of `tools/workshop/model/` on the whole body or
comment, and the text is cut after: one definition, shared with `workshop`.

An issue is a Workshop issue when it carries exactly one of the two kind
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
  suggestion, even one that failed halfway) starts one
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
  lists are read, and the next pages are asked for only then. Each comment
  keeps its 👍 count from the reaction summary GitHub returns with it. The
  refreshes in between ask by update time, and this code does not count on a
  👍 changing a comment's update time: a 👍 given on GitHub itself shows at
  the next full reload, within 10 minutes. One given through the Workshop
  shows at once, from the patch. When an issue
  list or the comment listing fails, the refresh's other calls are aborted,
  and the refresh fails with that first failure.
- **Writes.** After a signed-in person's write succeeds, the snapshot is
  patched in place: the new comment, the comment and 👍 counts, a comment's
  👍 count, a new suggestion. Each patch is kept
  with its time. When a refresh completes, GitHub's data replaces the snapshot
  and the patches made after that refresh began are applied again; older
  patches are dropped, since GitHub's answer already holds them. A count moves
  only from the value it had when the write happened, so a write GitHub
  already shows is not counted twice. An entry's thread the App has just
  opened is put in at once and kept by a different rule, given under
  "Writing to an entry": GitHub's list may not show it yet.
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

The installation token is spent only on the snapshot refresh, the repository
lookup at sign-in, labelling a suggestion, and opening an entry's thread (with
the reads of entry issues before and after it). Each has
its own ceiling per instance, so that no use can starve another, and nothing
a visitor does without signing in can make `suggest`,
`comment`, `vote` or `me` answer 429:

| Use | Calls a minute |
| --- | --- |
| Snapshot refresh | 40 |
| Repository lookup at sign-in (once per instance, then remembered) | 4 |
| Labelling a suggestion, and the label retries (charged per call) | 10 |
| Opening an entry's thread: the reads of entry issues and the opening (charged per call) | 27 |

Worst case per instance per hour: (40 + 4 + 10 + 27) × 60 = 81 × 60 =
4,860 calls, below the installation's 5,000. The calls that repair an opening
come on top (see below): at most 3 for an opening GitHub returned without its
label, two `POST`s and one `PATCH`. The largest refresh is 20 issue
pages + 15 comment pages = 35 calls, which fits in a minute's 40. GitHub's
secondary limit is 900 points a minute, a `GET` costing 1 point and a `POST`
5: at most 37 of the 81 calls a minute are `POST`s (labels and openings),
which is 37 × 5 + 44 = 229 points, plus 3 calls (15 points) for each
opening repaired in that minute. Calls to GitHub's token endpoint are not installation calls;
new sign-ins have a ceiling of 30 a minute and token refreshes of existing
sessions a separate one of 60, so made-up sign-ins cannot stop sessions from
refreshing.

The opening ceiling is 27 calls a minute per instance, whatever they are: the
reads of entry issues, the openings, and the calls that repair an opening. An
opening is 3 calls (the read before, the opening, the read after), so an
instance opens at most 9 threads a minute.

Two things bound the threads the App opens. An entry has one thread, so the
plan's entries are the most it can open. The `open` ceiling bounds how fast.
The opening and the first page of the read after it are charged
together, before the opening. When the ceiling has no room for both, the
answer is 429, "The Workshop is busy. Try again in a minute.", with
`Retry-After` for the wait, and nothing is opened. A further page of that read
is charged when it is read.

The calls that repair an opening (creating the missing label, adding it,
closing an issue) are charged to the ceiling but never refused by it, so that
nothing is left half done. The read before an opening also runs for a write on
an entry that the snapshot holds no thread for, and finds the thread when
another instance opened it.

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
two instances spending at their ceilings (4,860 an hour each) use it up in
about 31 minutes. Nothing in this code can prevent that: instances do not
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
- Each signed-in person may make 20 writes a minute per instance. Sign-out
  counts as a write, and so does a vote on a comment. A person may make 3
  suggestions a minute and 10 an hour; an attempt counts only when the issue
  was created.
- Each session may make 60 GitHub calls a minute for reads (`GET vote`, its
  GraphQL query, and the reads before a write); more answer 429. The reads a whole action needs are checked before its first
  call.
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
  Steps are ordered so that a cut is safe to retry: `comment` skips
  posting when the same person posted the identical text on that issue in the
  last five minutes.
- `me` reads the cookie and calls nothing on GitHub; it never refreshes the user
  token.
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
| `website/server/issues.mjs` | Kinds, the markers (read with `tools/workshop/model/`), which comments are the App's own, and the cuts. |
| `website/server/entries.mjs` | Reads `plan-entries.json`, the plan's entries the server knows. |
| `website/server/snapshot.mjs` | The in-memory snapshot that answers every anonymous read, and its refresh. |
| `website/server/workshop.mjs` | Every endpoint, independent of the HTTP server. |
| `website/server/node.mjs` | Adapter for Node's `req`/`res`, used by Vercel and the local server. |
| `website/api/workshop.cjs` | Vercel's entry point. |
| `website/scripts/prepare-vercel.mjs` | Writes the Build Output (static site, function, routes). |
| `website/scripts/plan-entries.mjs` | Makes the list of the plan's entries from `plan/`, for `prepare-vercel.mjs` and the local server. |
| `website/scripts/workshop-dev.mjs` | Local server for the site and the API. |
| `website/scripts/fake-github.mjs` | Fake GitHub used by the tests and by `--fake`. |
| `website/scripts/workshop-test-support.mjs` | Shared test setup: the fake GitHub on a controllable clock and a client with a cookie jar. |
| `website/scripts/workshop-server.test.mjs` | Endpoints, sign-in, sessions, refusals, configuration, packaging and the local server. |
| `website/scripts/workshop-snapshot.test.mjs` | The snapshot: reads without GitHub calls, conditional refreshes, changes, patches, bounds. |
| `website/scripts/workshop-limits.test.mjs` | Ceilings, `me`, sign-out, deadlines, patch timing, cleaned output, the App not installed. |
| `website/scripts/workshop-refresh.test.mjs` | Refreshes started by writes, coalescing, the floor, the idle refresh, what each read waits for and its headers, the parallel refresh and its failures. |
| `website/scripts/workshop-abuse.test.mjs` | What a visitor or a replayed cookie can make the server do, memory, waits and retries. |
| `website/scripts/workshop-threads.test.mjs` | Entries' threads: opening, finding, sharing, the `open` ceiling, failures, closed threads, `suggested_in`; votes on comments. |

All of them run with the website's `npm test`.

The server uses Node's standard library (`crypto`, `fetch`, `http`) and no
package. `prepare-vercel.mjs` refuses a file the function would load from
`website/node_modules/`, and the whole function is 15 files, about 170 KB, of which 70 KB are `plan-entries.json`.

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
  any depth, and the Workshop's
  model, `tools/workshop/model/*.js` with `tools/workshop/package.json` (which
  makes it ESM). The script finds what loads by loading the entry point in a separate
  process with a resolve hook, and refuses by name, before writing anything,
  any other file it loads, in `website/` or outside it. Beside the server it
  writes `website/server/plan-entries.json`, the plan's entries made from
  `plan/` (or the plan `WORKSHOP_PLAN` names), read and checked with
  `tools/plan` as the page's build reads them; a plan with a problem stops
  the script before it writes anything. The other
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
`amontlabs/silo` only ("Only select repositories"). `workshop setup`
(`tools/workshop`) does all of this from a page on `127.0.0.1` and stores the
credentials; `workshop store` finishes a setup whose Keychain step failed. The
repository needs the label `workshop:suggestion`, which the site puts on each
new suggestion; `workshop doctor` checks the credentials, the installation, the
permissions and that label. It also needs the label `workshop:entry`, which
the site puts on each entry's thread it opens; `workshop doctor` checks it too.
`workshop decline <issue> --why "…"` closes a suggestion as
not planned, with the reason as the bot's comment.

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
the repository lookup at sign-in, labelling a suggestion
(which a visitor cannot do) and opening an entry's thread, with the reads of
entry issues before and after it. Everything else done for a signed-in person,
including reading the issue before a write, uses that person's token, so
GitHub's own permissions apply (a locked issue refuses a visitor's comment)
and GitHub records the person as the author. Every write first reads the issue
and refuses with 404 unless it carries a Workshop label and is not a pull
request. A 401 on the installation token is a 502 and
drops the cached token; it never signs anyone out. *Limit:* if labelling a
suggestion fails twice, the new issue stays outside the Workshop until a
background retry labels it (in memory, on that instance only), and the answer
carries its number.

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
follows the symlinked functions and loads the `.cjs` handler. On 2026-11-20
it showed that the App opens an entry's issue with its label on the real
GitHub. Everything else was run against the fake GitHub and in Node. These still depend on the real
services:

- the token exchange body format: the server sends JSON with
  `Accept: application/json`, including `code_verifier` and `repository_id`;
- whether a refreshed user token stays limited to the repository;
- a visitor removing their own 👍 with their user token;
- that `waitUntil()` of the request context keeps a Build Output
  Node.js function alive after the response, as documented for Vercel
  Functions (the code does not depend on it);
- that a Vercel build gets the whole history (its clone is shallow, with no
  remote), until a deploy has shown it;
- that GitHub answers `304` to the conditional issue and comment list pages as
  documented, and that the reaction summary in the issue list moves the page's
  ETag when a vote changes.

## Why built this way

- **giscus** stores comments in GitHub Discussions and has no place for a
  vote that the page counts.
- **utterances** signs people in with an OAuth App that needs the broad
  `public_repo` scope, write access to every public repository of the person.
  A GitHub App's user token is limited to the App's permissions and, here, to
  one repository.
- **A database of our own** is something to run, secure and back up. Issues
  keep the history, the authorship and the moderation tools on GitHub.
- **Verdicts in the plan's files only.** The owner takes or drops an entry with
  `plan take` and `plan drop`, which commit the file; the site has no verdict
  route, so the App's key, held by Vercel, never gets write access to the code.
  See `tools/plan/README.md`.
