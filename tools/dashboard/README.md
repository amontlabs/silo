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
the repository:

- `dashboard.db`: SQLite (WAL) with items, replies, attachments and what the
  owner has seen. The CLI and the server open it directly, so the CLI works
  when the server is not running. A database from a newer version of the
  dashboard is refused, not changed.
- `attachments/<item-id>/<file>`: copies of attached files.
- `private/`: the owner's private Markdown notes, kept out of the public
  repository. Every `.md` file under it (folders included, dot files skipped)
  shows in the page's Private notes section, grouped by folder and sorted by
  file name. An empty or missing folder hides the section. A symbolic link
  that points outside `private/` is skipped.

## The page

One page with a sticky top bar:

1. **For you**: open items waiting for the owner (decide, then look, then
   shipped; newest first), and the product-doc sections that changed since the
   owner last pressed **Mark all as seen**.
2. **Product**: `PRODUCT.md`.
3. **Journeys**: one card per journey in `JOURNEYS.md`, with step counts and
   the items posted for it. Items with an unknown journey show under Other.
4. **Decisions**: `DECISIONS.md`, with a text filter.
5. **Private notes**: only when `$SILO_DASHBOARD_HOME/private/` has notes.
6. **History**: answered and closed items.

An item leaves For you when the owner replies after the latest agent reply,
marks a shipped item seen, or the item is closed. A new agent reply brings it
back.

"Changed since you last looked" compares each `## ` section of the three
product files with its text at the commit the owner marked as seen. Uncommitted
edits count. With no recorded commit, or one this checkout no longer has, every
section shows as new.

## JOURNEYS.md format

```markdown
# Journeys

Anything before the first journey is an introduction.

## First run
<!-- id: first-run -->

Ordinary Markdown about the journey.

- [built] Install the app
- [partial] Start a first computer
- [missing] A guided tour
- [untested] Restore a computer
```

- Each `## ` heading is one journey.
- An optional `<!-- id: slug -->` line right under the heading gives its id;
  otherwise the id is the heading, lowercased with runs of other characters
  turned into `-` ("Share a workspace" is `share-a-workspace`). Agents pass
  this id as `--journey`.
- A list item that starts with `[built]`, `[partial]`, `[missing]` or
  `[untested]` is a step and shows as a status chip; the card counts each
  status. `untested` means the docs or code say it exists but nobody has
  driven it as a user yet. Any other `[word]` stays plain text.
- ` ```mermaid ` blocks render as diagrams.

`DECISIONS.md` is a list: one list item per decision. `## ` headings group
them.

## The CLI

`tools/dashboard/bin/dashboard <command>`. Every command takes `--json` for
machine output. Errors go to stderr with exit 1; usage errors exit 2. `--body`
and `--text` take literal text, `@file` to read a file, or `-` for stdin.

Item kinds:

- `look`: an agent made a reversible choice. `--default` says what stands if
  the owner says nothing. The owner agrees, disagrees (with a reason) or
  comments.
- `decide`: agents could not pick; the owner answers in words.
- `shipped`: what a build delivered; the owner marks it seen.

```sh
# Post an item; prints its id.
dashboard post --kind look --title "Sidebar lists computers by last use" \
  --journey first-run --body @notes/sidebar.md \
  --default "Sorted by last use; alphabetical is one setting away" \
  --source feat/sidebar --attach shot.png:"The new sidebar"
dashboard post --kind decide --title "Name for the shared folder" --body - < question.md
dashboard post --kind shipped --title "0.42.0 build" --attach demo.mp4 --json   # {"id":3}

# Add a file to an item (a name already used gets -2, -3, ...).
dashboard attach 3 recording.mov --caption "Fork, then merge"

# Reply as an agent; the item comes back to For you.
dashboard reply 2 --text "Two options left: Shared and Common."

# Owner replies not yet acknowledged (--all includes acknowledged ones).
dashboard answers
dashboard answers --all --json

# Mark owner replies acknowledged once acted on.
dashboard ack 7 8

# Close an item, list items, show one.
dashboard close 1 --note "Done in feat/sidebar"
dashboard list            # open items (same as --open)
dashboard list --all
dashboard show 2
```

## Security

Local only, no authentication:

- The server listens on 127.0.0.1 and answers only requests whose `Host` is
  its own `127.0.0.1:<port>` or `localhost:<port>`. A write (any non-GET
  request) must be JSON and is refused when its `Origin` is another origin or
  the browser marks it cross-site.
- Item bodies, defaults and replies are Markdown from agents and the owner:
  raw HTML in them is escaped and links keep to `http`, `https`, `mailto` and
  relative targets. Product docs and private notes render as written.
- Attachments are served only when the database records them, with the
  recorded media type and range support. Images, video and audio display
  inline; anything else downloads.
- No request path reaches outside `docs/product/`, the store or the served
  assets.
