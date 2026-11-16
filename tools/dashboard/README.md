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
  in place, in one transaction, keeping every id. Every checkout that opens
  the same store must therefore run the same dashboard version: once a newer
  one has upgraded it, an older copy refuses to open it.
- `attachments/<item-id>/<file>`: copies of attached files.
- `private/`: the owner's private Markdown notes, kept out of the public
  repository. Every `.md` file under it (folders included, dot files skipped)
  shows in the page's Private notes section, grouped by folder and sorted by
  file name. An empty or missing folder hides the section. A symbolic link
  that points outside `private/` is skipped.

## The page

A sticky top bar with one tab per view, in this order: **Stack**, **For you**,
**Decisions**, **Product**, and **Private** when there are private notes. On
the right, the short commit the product documents were read at. One view shows
at a time, chosen by the address (`#stack`, `#for-you`, `#decisions`,
`#product`, `#private`, or an anchor inside one, such as `#feature-<id>`,
`#item-N` or `#product-<section key>`). With no address or an unknown one the
page opens the Stack when `docs/product/features.jsonl` exists, else For you.
Without JavaScript every view shows, in that order.

1. **Stack** (the home): where the work stands, then the feature list one
   entry at a time, starter first; the tab's badge counts entries without a
   verdict. Only when `features.jsonl` exists. See [The Stack](#the-stack).
2. **For you**: the items waiting for the owner, one card at a time, in one
   column, in order (decide, then look, then shipped, then notes an agent
   replied to; newest first). "N things · about M minutes" (M = 0.6 × N,
   rounded, at least 1) and progress dots above it. The tab shows a count only
   when something waits. Each card has its actions:
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
   of the same items. With nothing waiting: a tick, "Nothing is waiting for
   you" and **Go to the Stack**. Answered and closed items sit below the queue
   behind one closed disclosure, **Earlier items (N)**. A card about a list
   entry says "on <entry name>", linking to the entry in the Stack (the raw id
   when the entry is not on the list).
3. **Decisions**: everything decided, in one timeline. See
   [Decisions](#decisions).
4. **Product**: the "In one sentence" section of `PRODUCT.md` (else the
   file's first paragraph) as the headline, a built-in animation of the loop,
   then every other `## ` section as a tile with **＋ note**. Who it is for,
   The problem, The loop and Rules that hold everywhere take two columns.
   "Changed since you last looked" is a thin strip at the top (see below).
5. **Private**: only when `$SILO_DASHBOARD_HOME/private/` has notes. A tile per
   note; a tile opens the note in a reader with a table of its `## ` headings.

An item leaves For you when the owner replies after the latest agent reply,
marks a shipped item (or a note) seen, or the item is closed. A new agent reply
brings it back.

### Notes on product sections

The owner can say something about any section of `PRODUCT.md` without being
asked: each tile has **＋ note**. Each section has one open item of kind
`note`, created by the owner; every note is an owner reply on it, so agents
read them with `dashboard answers`. A note shows in For you only after an
agent replies to it ("An agent replied to your note"). A tile shows "N notes
from you" with the thread.

The anchor is `product#<section key>`; `/api/notes` refuses any other. A
section key is its heading, lowercased with runs of other characters turned
into `-` (a second section with the same heading gets `-2`), at most 80
characters. The item also keeps `anchor_text`, the heading as the page showed
it (at most 200 characters). A note is refused (409, and the page shows it
again) when its section is no longer in `PRODUCT.md` or its heading changed
since the page showed it. A note whose section was renamed or removed is kept:
Product ends with "Notes on things that changed", with its thread and a box to
reply. Note items an earlier version stored on journey anchors (`journeys#…`)
stay in the store, in `dashboard answers` and in the Decisions timeline, by
their anchor text.

Two safeguards on the queue: a card that has just appeared takes no Enter
or primary click for 0.7 s (its primary button shows it), and a held key does
not repeat. These rules live in `public/rules.js`, which `npm test` runs
without a browser. The address always names the card on screen (`#item-N`).
Text whose send did not go through is never dropped: it stays in its box, or,
when the box is gone because the item was closed or answered meanwhile, it
shows in a notice at the top of the view on screen, to copy or dismiss. A
refused note always shows that notice ("your note was not recorded", with the
text), whether or not its box is still on the page.

"Changed since you last looked" compares each `## ` section of `PRODUCT.md`,
`DECISIONS.md` and `RESTART.md` with its text at the commit the owner marked
as read. Uncommitted edits count. Before the first **Mark as read** (or when
this checkout no longer has the marked commit) there is nothing to compare
with: the strip is one line, "Mark as read to see what changes from now on",
and the button. After that it lists the changed sections, each expanding to
its before and after, with **Mark all as seen**; with nothing changed there is
no strip.

### The Stack

Above the lede, a "Where we are" strip, read live from
[`RESTART.md`](#restartmd-format): the Done steps folded into one chip, "N
done", that opens to show them; the Now step in full; the Next and Then steps
muted, first sentence only, in order. It answers "what do I do now".

The owner goes down the feature list from the top and gives each entry a
verdict in one click: **Take**, **Refine…** (a note saying what to change is
required), **Later** or **Drop**. The button matching the entry's suggestion
is the filled one and Enter presses it. **Add a note** (or Tab) lets any
verdict carry a note: the box never sends by itself, and Cmd+Enter there only
moves to the verdict buttons ("Now pick a verdict"). In the box **Refine…**
opens, Cmd+Enter sends Refine. Plain Enter in a box is a new line; Escape
closes it. → skips to the next entry without a verdict, ← goes back one entry.
After a verdict the card leaves and the next entry without one enters. An entry
with a verdict shows it as a chip with its note, **Change** (the buttons again)
and **Clear**, which clears the verdict and keeps any text typed in the box. A
verdict that comes only from `features.jsonl` is marked "carried over", not as
the owner's, with **Confirm**, which records the same verdict as the owner's
own.

The same safeguards as For you, in both queues: a card waits 0.7 s from the
moment the owner can see it (its view is on screen and it is the current card:
opening the tab, a stage jump and a See all click start the wait again, and so
does new text on the same entry), a held key does not repeat, and from the
moment a move or an answer starts until the next card is current every action
key and button is refused. These rules live in `public/rules.js`.

Each card carries a digest of what it shows (name, what, old, suggest, why;
SHA-256, first 16 hex characters). A verdict sent for text that has changed
since, or for an entry no longer in the list, is refused: the Stack reloads and
says "This entry changed or was removed; your verdict was not recorded", with
the note when there was one.

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

## RESTART.md format

The strip reads the ordered list under the `## Where the work stands` heading
(any case), outside code fences:

```markdown
## Where the work stands

1. Done: the old tree is frozen.
2. Now: the owner goes down the Stack and gives verdicts.
3. Next: start `main` again. Wait for the owner first.
4. Then: design the shell's records, calls and events.
```

Each item starts with a state word and a colon: `Done`, `Now`, `Next` or
`Then`, in any case. The rest is inline Markdown, escaped like every other
source. An item with another first word shows muted, like Then, with its whole
first sentence. No file, no heading or no list: no strip.

## Decisions

One timeline of everything decided, newest day first. Each row has a small
kind label:

- **Decision**: a line of `DECISIONS.md`, `- YYYY-MM-DD · who · what`, split
  on the first two ` · `. `owner` shows as "you"; any other value shows as
  written. The text is inline Markdown. An item without that shape shows in a
  group, Other. Text outside list items is not shown.
- **Verdict**: every verdict the owner gave on the Stack, as history: each
  row of the store on its own day. A day's verdicts fold into one row that
  counts what was done that day, "You took 6 entries, kept 1 for later,
  dropped 2, cleared 1", and opens to the entries (name, verdict, note), each
  linking to its card in the Stack. A change names the verdict it replaced,
  struck ("~~Taken~~ changed to dropped"); a clear reads "cleared its
  verdict"; the same verdict as the file's reads "confirmed".
- **From the list**: entries whose verdict comes only from `features.jsonl`
  (no row in the store at all) form the last group, "Carried over from
  earlier decisions". It never says "you": its pill reads "carried over" and
  its text "13 entries taken, 3 dropped, from decisions made before the
  Stack"; each entry shows its `why`.
- **Answer**: each owner reply on an item (agreed, disagreed, a chosen option,
  free text, a note on a product section): the item's title and what was
  answered, linking to the item.

Chips filter by kind (All, Decisions, Verdicts, Answers); the search box
filters every kind. The tab's count is the number of `DECISIONS.md` lines.

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

`--entry ID` names the list entry an item is about (its `id` in
`features.jsonl`); the card links to it in the Stack.

```sh
# Post an item; prints its id.
dashboard post --kind look --title "Sidebar lists computers by last use" \
  --entry engine-service --body @notes/sidebar.md \
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
# choice_label), and the owner's notes (anchor, anchor_text, text). Notes an
# earlier version stored on journey steps may carry a reaction.
dashboard answers
dashboard answers --all --json

# Mark owner replies acknowledged once acted on.
dashboard ack 7 8

# The owner's verdicts on the feature list not yet acknowledged, one line each
# starting with what to acknowledge (FEATURE_ID@ID), then the verdict ("cleared"
# when the owner cleared one), the entry's name as the owner judged it, when,
# and the note. --json gives id, ack, feature_id, name, in_list, verdict, note,
# created_at, acked_at. --all lists every current verdict.
dashboard verdicts
dashboard verdicts --all --json

# Acknowledge them once written into features.jsonl and DECISIONS.md, exactly
# as listed: FEATURE_ID@ID acknowledges that row and older ones, so a verdict
# the owner gave after you listed stays unread. A bare FEATURE_ID is refused.
dashboard verdicts ack engine-service@3 vm-runtime@12

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
  decoded; anything else renders as plain text. In documents of the checkout
  (`PRODUCT.md`, `DECISIONS.md`, `RESTART.md`, `what` and `why` in
  `features.jsonl`) only absolute `http`, `https` and `mailto` links stay: a
  relative link names a file the dashboard does not serve, so it shows as its
  text. The page's script acts only on
  controls inside the wrappers the server renders for items, notes and Mark
  as read. Scripts are files (`/boot.js`, `/app.js`); the page has no
  inline script or event handler attributes.
- `POST /api/features/<id>/verdict` takes `{verdict, note?, shown, seen}`:
  `verdict` is `take`, `refine`, `later`, `drop` or null (clears it), `shown`
  is the id of the newest verdict row the page showed for that entry (0 for
  none) and `seen` the digest of the entry's text it showed. An entry no
  longer in `features.jsonl`, other text, or a newer row answers 409 and the
  page shows the Stack again; Refine needs a note. Each row also keeps the
  entry's name as it was judged. Every
  text from `features.jsonl` and `stages.json` is escaped; `what` and `why`
  go through the same inline Markdown renderer as the documents.
- `POST /api/notes` takes `{anchor, anchorText, text, after}`; the anchor
  must be `product#<section key>` and the text is required.
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
