# Owner dashboard

The owner's single page for the Silo product: it shows the product documents in
`docs/product/` and the items agents post for the owner, and takes
the owner's answers. It runs locally from [`tools/dashboard/`](../tools/dashboard/README.md);
the README there has the commands, the `RESTART.md`, `DECISIONS.md` and
`features.jsonl` formats, and where data lives.

It is a development tool, not part of the Silo app. It has no changeset, does
not follow the app's vocabulary rules, and never touches a Silo engine or app.

## What it is for

Silo is being restarted from an empty tree ([RESTART.md](product/RESTART.md)),
and the dashboard is about that restart only. Agents build most of Silo. Some
of what they do needs the owner: a verdict on an entry of the feature list, a
choice the owner may want to reverse, a question agents cannot settle, a build
worth seeing. The dashboard puts them on one page next to the product
description and everything decided, so each answer is given with the product
in view, and agents read the answers back from the command line.

One obvious starting point and three things to look at:

- **Stack** (the home): "Where we are", read from the "Where the work stands"
  list of `RESTART.md`, then the feature list one entry at a time for the
  owner's verdicts.
- **For you**: a plain queue of the items agents posted, one card at a time;
  earlier items behind one disclosure.
- **Decisions**: the one place for everything decided: the lines of
  `DECISIONS.md`, the owner's verdicts on the list (every change and clear
  kept as history; verdicts that come only from `features.jsonl` are marked
  "carried over", never as the owner's), and the owner's answers on items, in
  one timeline.

**Product** shows `PRODUCT.md`, takes notes on its sections, and says what
changed in `PRODUCT.md`, `DECISIONS.md` and `RESTART.md` since the owner last
marked them read. **Private** shows the owner's private notes when there are
some.

## How agents use it

Agents call `tools/dashboard/bin/dashboard` (add `--json` for machine output).
The store lives outside the repository, so posting never changes the checkout
and never conflicts with a merge.

1. **Post** what needs the owner, tagged with the list entry it is about when
   one fits (`--entry FEATURE_ID`, an `id` from `docs/product/features.jsonl`;
   the card links to the entry in the Stack):
   - `look`: a reversible choice an agent made. `--default` says what stands
     if the owner says nothing; work goes on with the default.
   - `decide`: agents could not pick. Work that depends on it waits. When
     the answer is one of a few choices, offer them: `--option
     "Label::Detail"` for each (2 to 6) and `--recommend N` for the one you
     would pick. The owner answers with one click; free text stays possible.
   - `shipped`: what a build delivered, with screenshots or a recording
     (`--attach`). Information only.
2. **Read answers** with `dashboard answers`. Each line names the reply, the
   item, the owner's verdict (agree or disagree, for a look item), the option
   picked (`chose N "Label"`; `choice` is 0-based and `choice_label` is its
   label in `--json`) and text.
   The same command returns the owner's own notes: items of kind `note`,
   which the owner creates from the page on a section of `PRODUCT.md`. Each
   carries `anchor` (`product#<section key>`, the heading slugified),
   `anchor_text` (what the owner saw) and the text. Notes an earlier version
   stored on journey steps keep their `journeys#…` anchor and may carry a
   `reaction`. Treat a note like any answer: act on it, ack it, and reply or
   close it when there is something to say; a reply puts it in the owner's
   For you list.
3. **Act on them**, then `dashboard ack REPLY_ID...` so the answer is not
   picked up twice. Reply with `dashboard reply ID --text ...` when the owner
   needs to know more; the item returns to the owner's For you list.
4. **Close** an item once it is settled: `dashboard close ID --note ...`.

### The feature list and its verdicts

`docs/product/features.jsonl` is the ordered list of every feature Silo had,
wants or considered, one JSON object per line, with `stages.json` naming the
stages; the README lists the fields. The orchestrator writes both. The
dashboard's Stack view shows the list one entry at a time and the owner gives
each a verdict: take, refine (with a note saying what to change), later or
drop, any of them with a note. Verdicts are kept in the dashboard store, never
in the repository.

1. `dashboard verdicts` lists the current verdicts no agent has acknowledged:
   `FEATURE_ID@ID` to acknowledge, verdict (cleared when the owner cleared
   one), the name the owner judged, when and the note (`--json` for machine
   output, `--all` for every current verdict).
2. The orchestrator writes each into `features.jsonl` (`verdict`) and records
   it in `DECISIONS.md`, by hand, in one commit.
3. Then `dashboard verdicts ack FEATURE_ID@ID...`, exactly as listed, so it is
   not picked up twice. A bare feature id is refused: it could acknowledge a
   verdict the owner gave after you read the list. A later change by the owner
   shows up again.

The page is built to take the owner's input with little reading and few
clicks: one card at a time, options to pick, one click per verdict. Keep
titles short, put what stands in `--default`, and offer options when you can.

The product documents are written by the orchestrator. The dashboard reads
them live and shows which sections changed since the owner last marked them
read; it never writes to them. Keep the "Where the work stands" list of
`RESTART.md` current: each item starts with `Done:`, `Now:`, `Next:` or
`Then:`, and the Stack's strip shows it. Owner documents that must stay out of this
public repository go in `$SILO_DASHBOARD_HOME/private/`, which the dashboard
shows as Private notes.

## Why in-house

Before building, these existing tools were considered. They were assessed from
their public descriptions, not trialled.

- **Agent inboxes (Placet, Pinrail)**: they collect agent requests and the
  human's answers, but have no product view: the answers would live apart
  from the product description, feature list and decisions they are about.
- **Backlog.md**: keeps tasks and answers as Markdown files committed to the
  repository. An owner answer would be a commit on `main`, which voids a
  running atelier merge and races the agents committing to the same checkout.
- **Documentation site generators**: render the product documents well but
  have no items, answers or "changed since you last looked".

The dashboard is small (Node's built-in HTTP server and SQLite, `marked` for
Markdown, `mermaid` for diagrams), so owning it costs little. Revisit this if
it grows past one page and a handful of commands.
