# Owner dashboard

A small local web page and a command-line tool. The owner reads the product
documents (`docs/product/`) and answers the items that agents post. Why it
exists and how agents use it: [docs/OWNER-DASHBOARD.md](../../docs/OWNER-DASHBOARD.md).

It is a development tool, not part of the Silo app. It never touches a Silo
engine or app.

## Run it

Node 26. From the repository root:

```sh
npm --prefix tools/dashboard ci
tools/dashboard/bin/dashboard serve          # http://127.0.0.1:4517/
npm --prefix tools/dashboard test            # node --test
```

`serve` binds 127.0.0.1 only. `--port N` picks another port (`--port 0` picks a
free one); a port in use is an error. The page reads `docs/product/` from the
checkout the server runs from, on every request; set `SILO_DASHBOARD_REPO` to
read another checkout. Stop it with Ctrl-C.

## Where data lives

`$SILO_DASHBOARD_HOME`, default `~/.local/state/silo-dashboard/`, never inside
the repository. Each open sets the folder and attachment folders to 0700 and
the database files and attachments to 0600, also when they existed with wider
modes:

- `dashboard.db`: SQLite (WAL) with items, replies, attachments, what the
  owner has seen, and the owner's verdicts on the feature list (schema
  version 3). The CLI and the server open it directly, so the CLI works
  when the server is not running. A database from a newer version of the
  dashboard is refused, not changed; one from an older version is upgraded
  in place, in one transaction, keeping every id.
- `attachments/<item-id>/<file>`: copies of attached files.
- `private/`: the owner's private Markdown notes, kept out of the public
  repository. Every `.md` file under it (folders included, dot files skipped)
  shows in the page's Private notes section, grouped by folder and sorted by
  file name. An empty or missing folder hides the section. A symbolic link
  that points outside `private/` is skipped.

## The page

A sticky top bar with one tab per view and, on the right, the short commit the
product documents were read at. One view shows at a time, chosen by the
address (`#for-you`, `#stack`, `#product`, `#journeys`, `#decisions`,
`#private`, or an anchor inside one, such as `#journey-live-step-3` or
`#feature-<id>`). Without JavaScript every
view shows, in order.

1. **For you** (default): the items waiting for the owner, one card at a time,
   in order (decide, then look, then shipped, then notes an agent replied to;
   newest first). "N things · about M minutes" (M = 0.6 × N, rounded, at least
   1) and progress dots above it. Each card has its actions:
   - look: **Fine, go ahead** (agree), **No, do this instead…** (disagree,
     with text), **Later** (nothing recorded).
   - decide with options: one button per option, the recommended one marked;
     **Pick the recommended** when there is one; **Say something…** answers
     in words.
   - decide without options: the text box is open; **Send**.
   - shipped, and a note an agent replied to: **Got it** (seen),
     **Say something…**.

   Keys, when the focus is not in a text field: Enter is the primary action,
   → is Later, ← goes back, Tab opens the text box. In the text box Cmd+Enter
   or Ctrl+Enter sends and Escape closes it. **See all** switches to a list
   of the same items. **Answered** below lists answered and closed items,
   collapsed. The side column has one ring per journey (built, then partial
   share of its steps) and "Changed since you last looked": each changed
   section expands to its before and after, and **Mark all as seen** clears
   them. A line links to the Stack with the number of entries without a
   verdict.
2. **Stack**: only when `docs/product/features.jsonl` exists. The feature
   list one entry at a time, starter first; the tab's badge counts entries
   without a verdict. See [The Stack](#the-stack).
3. **Product**: the "In one sentence" section of `PRODUCT.md` (else the
   file's first paragraph) as the headline, a built-in animation of the loop,
   then every other `## ` section as a tile. Who it is for, The problem, The
   loop and Rules that hold everywhere take two columns.
4. **Journeys**: a card per journey with its steps, a status filter (All or
   Only gaps: everything not built), the open items posted for it and the
   rest of its text behind **More**.
5. **Decisions**: `DECISIONS.md` as a timeline grouped by date, with a search
   filter.
6. **Private**: only when `$SILO_DASHBOARD_HOME/private/` has notes. A tile per
   note; a tile opens the note in a reader with a table of its `## ` headings.

An item leaves For you when the owner replies after the latest agent reply,
marks a shipped item (or a note) seen, or the item is closed. A new agent reply
brings it back.

### Owner notes and reactions

The owner can say something about any journey step, journey or product section
without being asked: an open step has "For you this is" **essential**, **nice
to have** or **not needed** (click again to clear) and **＋ note**; a journey
and each product tile have **＋ note**. Each anchor has one open item of kind
`note`, created by the owner; every note or reaction is an owner reply on it,
so agents read them with `dashboard answers`. A note shows in For you only
after an agent replies to it ("An agent replied to your note"). A step shows
its reaction and a mark when it has notes; the open step shows the thread.

Anchors: `journeys#<journey id>/<step key>`, `journeys#<journey id>`,
`product#<section key>`. A step key is its title, lowercased with runs of
other characters turned into `-` (a second step with the same title gets
`-2`); a section key is made the same way from its heading. Ids and keys are
lowercase letters, digits and hyphens, at most 80 characters: a longer one is
cut, and an `<!-- id: -->` that does not fit is ignored (the heading gives the
id). A step without a title is "Step N". Because a step's key comes from its
title, a note stays with its step when steps are added or moved. The item
also keeps `anchor_text`, the title as the page showed it (at most 200
characters). A note is refused (409, and the page shows it again) when its
anchor is no longer in the documents or its title changed since the page
showed it. A note whose step or section was renamed or removed is kept:
Journeys and Product end with "Notes on things that changed", with its
thread and a box to reply.

Two safeguards on the queue: a card that has just appeared takes no Enter
or primary click for 0.7 s (its primary button shows it), and a held key does
not repeat. These rules live in `public/rules.js`, which `npm test` runs
without a browser. The address always names the card on screen (`#item-N`).
Text whose send did not go through is never dropped: it stays in its box, or,
when the box is gone because the item was closed or answered meanwhile, it
shows in a notice at the top of the view on screen, to copy or dismiss. A
refused note or reaction always shows that notice ("your note/reaction was
not recorded", with the text when there is one), whether or not its box is
still on the page.

"Changed since you last looked" compares each `## ` section of the three
product files with its text at the commit the owner marked as seen. Uncommitted
edits count. With no recorded commit, or one this checkout no longer has, every
section shows as new.

### The Stack

The owner goes down the feature list from the top and gives each entry a
verdict in one click: **Take**, **Refine…** (a note saying what to change is
required), **Later** or **Drop**. The button matching the entry's suggestion
is the filled one and Enter presses it; the same 0.7 s wait and held-key rule
as For you apply. **Add a note** (or Tab) lets any verdict carry a note; in the
box Cmd+Enter sends (Refine, or the suggestion) and Escape closes it. → skips
to the next entry without a verdict, ← goes back one entry. After a verdict the
card leaves and the next entry without one enters. An entry with a verdict
shows it as a chip with its note, **Change** (the buttons again) and
**Clear**.

Around the card: the stages drawn as a stack, the first at the bottom, each
with "decided/total" and one segment per entry coloured by verdict (a cut line
above Living in it marks the starter); clicking a stage jumps to its first entry
without a verdict. "Coming next" lists the next five entries without one.
**See all** shows the whole list by stage, with a filter (All, Undecided,
Taken: take and refine, Later, Dropped). "The product so far" draws the Mac app
and the command line from the entries taken (take or refine) and highlights what
the entry on screen would add. Below 1100 px it moves under the queue; below
860 px the stages become a strip above it.

The page shows the store's newest verdict for an entry when there is one, else
the `verdict` written in the file. A cleared verdict (a row with no verdict)
leaves the entry undecided, also when the file has one. Every verdict is kept as
history.

## features.jsonl and stages.json

`docs/product/features.jsonl` holds one JSON object per line, in list order,
written by the orchestrator. The dashboard reads it live and never writes it.

| field | meaning |
| --- | --- |
| `n` | order in the list (the page sorts by it, then by line) |
| `id` | lowercase letters, digits and hyphens; unique |
| `stage` | the id of a stage in `stages.json` |
| `name`, `what` | the title and one or two sentences (inline Markdown) |
| `old` | what the old code had: `driven`, `partial`, `built`, `planned`, `idea`, `dropped` |
| `old_lines` | rough size in the old code (0 or absent: not shown) |
| `reuse` | `reuse`, `rework`, `rewrite` or `unknown` |
| `suggest`, `why` | the suggested verdict (`take`, `refine`, `later`, `drop`) and the line behind it |
| `app` | where it shows in the app: `window`, `side:<label>`, `tab:<label>`, `button:<label>`, `menubar`, `progress`, `tree`, or null |
| `cli` | the commands it adds, or null |
| `verdict` | the owner's verdict as last written into the file, or null |

A line without a usable `id`, `name` or `stage`, a line that is not JSON, and a
second line with an id already seen are skipped; the page counts them in a
small warning. Unknown fields are ignored and an unknown value counts as not
given. A missing file hides the Stack. `stages.json` is `[{id, title, line}]`
in order; a stage a feature names that the file lacks is shown after the
others, titled by its id.

## JOURNEYS.md format

```markdown
# Journeys

Anything before the first journey is an introduction.

## 1. First run
<!-- id: first-run -->

The promise: one paragraph saying what the journey gives the user.

- [built] Install the app. Detail about the step, on as many lines as needed.
- [partial] Start a first computer. What works and what does not.
  - Nested content is part of the step's detail.
- [missing] A guided tour
- [untested] Restore a computer

Any other text: shown under the steps, behind More.
```

- Each `## ` heading is one journey. A leading `N. ` is its number; otherwise
  its position is.
- An optional `<!-- id: slug -->` line right under the heading gives its id;
  otherwise the id is the heading, lowercased with runs of other characters
  turned into `-` ("Share a workspace" is `share-a-workspace`). Agents pass
  this id as `--journey`. The ids `live`, `task` and `outside` have their own
  icons; others get a neutral one.
- The first paragraph under the heading is the journey's promise.
- A list item that starts with `[built]`, `[partial]`, `[missing]` or
  `[untested]` is a step. Its title is the text up to the first `. ` (or the
  whole first paragraph, without a final period); the rest, and anything
  nested under the item, is its detail. `untested` means the docs or code say
  it exists but nobody has driven it as a user yet. Any other `[word]` stays
  plain text.
- Everything else under the journey shows under the steps, behind More.
  ` ```mermaid ` blocks render as diagrams.

## DECISIONS.md format

One list item per decision: `- YYYY-MM-DD · who · what`, split on the first
two ` · `. `owner` shows as "you"; any other value shows as written. The text
is inline Markdown. Items grouped by date, newest first; an item without that
shape shows in a last group, Other. Text outside list items is not shown.

## The CLI

`tools/dashboard/bin/dashboard <command>`. Every command takes `--json` for
machine output. Errors go to stderr with exit 1; usage errors exit 2. `--body`
and `--text` take literal text, `@file` to read a file, or `-` for stdin.

Item kinds:

- `look`: an agent made a reversible choice. `--default` says what stands if
  the owner says nothing. The owner agrees, disagrees (with a reason) or
  comments.
- `decide`: agents could not pick. Up to 6 options (`--option "Label::Detail"`,
  label up to 80 characters, detail up to 300 and optional; at least 2), one
  of them recommended with `--recommend N` (from 1). The owner picks one or
  answers in words.
- `shipped`: what a build delivered; the owner marks it seen.

```sh
# Post an item; prints its id.
dashboard post --kind look --title "Sidebar lists computers by last use" \
  --journey first-run --body @notes/sidebar.md \
  --default "Sorted by last use; alphabetical is one setting away" \
  --source feat/sidebar --attach shot.png:"The new sidebar"
dashboard post --kind decide --title "Name for the shared folder" --body - < question.md
dashboard post --kind decide --title "After a task ends, what happens to its computer?" \
  --option "Remove it::Nothing piles up" --option "Stop it and keep it" \
  --option "Leave it running::As today" --recommend 1
dashboard post --kind shipped --title "0.42.0 build" --attach demo.mp4 --json   # {"id":3}

# Add a file to an item (a name already used gets -2, -3, ...).
dashboard attach 3 recording.mov --caption "Fork, then merge"

# Reply as an agent; the item comes back to For you.
dashboard reply 2 --text "Two options left: Shared and Common."

# Owner replies not yet acknowledged (--all includes acknowledged ones):
# answers, picked options ("chose 1 \"Remove it\"", JSON choice and
# choice_label), and the owner's notes (anchor, anchor_text, reaction, text;
# an empty text with no reaction clears the reaction).
dashboard answers
dashboard answers --all --json

# Mark owner replies acknowledged once acted on.
dashboard ack 7 8

# The owner's verdicts on the feature list not yet acknowledged: verdict row
# id, feature id, name, verdict (null when cleared), note, when. --all lists
# every current verdict.
dashboard verdicts
dashboard verdicts --all --json

# Acknowledge them once written into features.jsonl and DECISIONS.md.
# FEATURE_ID@ID acknowledges only rows up to verdict row ID, so a verdict the
# owner changed after you listed stays unread.
dashboard verdicts ack engine-service vm-runtime@12

# Close an item, list items, show one.
dashboard close 1 --note "Done in feat/sidebar"
dashboard list            # open items (same as --open)
dashboard list --all
dashboard show 2
```

## Security

Local only, no authentication and no access token. The dashboard keeps other
users of the Mac and web pages out; other processes running as the owner's own
account are out of scope (they can read the store directly anyway).

- The server listens on 127.0.0.1 and answers only requests whose `Host` is
  its own `127.0.0.1:<port>` or `localhost:<port>`. A write (any non-GET
  request) must be JSON and is refused when its `Origin` is another origin or
  the browser marks it cross-site.
- Every Markdown source (product docs, private notes, item bodies, defaults
  and replies) goes through one renderer that escapes raw HTML, so a document
  cannot add controls, styles or page regions. Links and images keep to
  `http`, `https`, `mailto` and relative targets, checked after entities are
  decoded; anything else renders as plain text. The page's script acts only on
  controls inside the wrappers the server renders for items, notes and Mark
  all as seen. Scripts are files (`/boot.js`, `/app.js`); the page has no
  inline script or event handler attributes.
- `POST /api/features/<id>/verdict` takes `{verdict, note?, shown}`:
  `verdict` is `take`, `refine`, `later`, `drop` or null (clears it), `shown`
  is the id of the newest verdict row the page showed for that entry (0 for
  none). The id must be in the current `features.jsonl` (404); a newer row
  answers 409 and the page shows the entry again; Refine needs a note. Every
  text from `features.jsonl` and `stages.json` is escaped; `what` and `why`
  go through the same inline Markdown renderer as the documents.
- `POST /api/notes` takes `{anchor, anchorText, journey?, text?, reaction?,
  after}`; the anchor must have one of the three shapes, and text or a
  reaction is required (`reaction: null` clears one).
- Each owner action sends what the page showed: the newest reply id of the
  item (for a note, of the anchor's open note item), or the docs commit for
  Mark all as seen. An item with a newer reply
  answers 409 and the page shows it again with a note to read it first; Mark
  all as seen records the commit the page was rendered from. The CLI does not
  send these.
- Attachments are served only when the database records them, with the
  recorded media type and range support. Images, video and audio display
  inline; anything else downloads.
- No request path reaches outside `docs/product/`, the store or the served
  assets.
