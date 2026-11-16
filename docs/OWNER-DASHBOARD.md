# Owner dashboard

The owner's single page for the Silo product: it shows the product documents in
`docs/product/` and the items agents post for the owner, and takes
the owner's answers. It runs locally from [`tools/dashboard/`](../tools/dashboard/README.md);
the README there has the commands, the `JOURNEYS.md` format and where data lives.

It is a development tool, not part of the Silo app. It has no changeset, does
not follow the app's vocabulary rules, and never touches a Silo engine or app.

## What it is for

Agents build most of Silo. Some of what they do needs the owner: a choice the
owner may want to reverse, a question agents cannot settle, a build worth
seeing. Before the dashboard these reached the owner as lines in long reports.
The dashboard puts them on one page next to the product description, the user
journeys and the recorded decisions, so each answer is given with the product
in view, and agents read the answers back from the command line.

## How agents use it

Agents call `tools/dashboard/bin/dashboard` (add `--json` for machine output).
The store lives outside the repository, so posting never changes the checkout
and never conflicts with a merge.

1. **Post** what needs the owner, tagged with a journey id from
   `docs/product/JOURNEYS.md` when one fits:
   - `look`: a reversible choice an agent made. `--default` says what stands
     if the owner says nothing; work goes on with the default.
   - `decide`: agents could not pick. Work that depends on it waits.
   - `shipped`: what a build delivered, with screenshots or a recording
     (`--attach`). Information only.
2. **Read answers** with `dashboard answers`. Each line names the reply, the
   item, the owner's verdict (agree or disagree, for a look item) and text.
3. **Act on them**, then `dashboard ack REPLY_ID...` so the answer is not
   picked up twice. Reply with `dashboard reply ID --text ...` when the owner
   needs to know more; the item returns to the owner's For you list.
4. **Close** an item once it is settled: `dashboard close ID --note ...`.

The product documents are written by the orchestrator. The dashboard reads
them live and shows which sections changed since the owner last marked them
seen; it never writes to them. Owner documents that must stay out of this
public repository go in `$SILO_DASHBOARD_HOME/private/`, which the dashboard
shows as Private notes.

## Why in-house

Before building, these existing tools were considered. They were assessed from
their public descriptions, not trialled.

- **Agent inboxes (Placet, Pinrail)**: they collect agent requests and the
  human's answers, but have no product view: the answers would live apart
  from the product description, journeys and decisions they are about.
- **Backlog.md**: keeps tasks and answers as Markdown files committed to the
  repository. An owner answer would be a commit on `main`, which voids a
  running atelier merge and races the agents committing to the same checkout.
- **Documentation site generators**: render the product documents well but
  have no items, answers or "changed since you last looked".

The dashboard is small (Node's built-in HTTP server and SQLite, `marked` for
Markdown, `mermaid` for diagrams), so owning it costs little. Revisit this if
it grows past one page and a handful of commands.
