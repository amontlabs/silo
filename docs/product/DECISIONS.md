# Decisions

One line per product decision: the date, who decided, and what. Newest first. Agents read this
before asking; a question answered here is not asked again. A decision that replaces an earlier
one says so. Decisions made before 2026-11-15 are in [the direction page](../DIRECTION.md); the ones the owner
confirmed are carried on the feature list as verdicts.

- 2026-11-16 · owner · The plan gets one source of truth as data: each fact is edited in one place, everything else is generated from it, and nothing can drift. Agents get one entry point to read a feature with all its context and to change it. The design is [PLAN.md](PLAN.md); five points in it still wait for the owner.
- 2026-11-16 · owner · The Workshop shows the plan as a map of the product's parts, with tiles on narrow screens; opening a part lists its entries. Visitors see the plan only; the decisions are shown to the owner. This replaces the single list in build order.
- 2026-11-16 · owner · A visitor who does not want to sign in on the Workshop is offered a link to comment on GitHub instead.
- 2026-11-16 · owner · The dashboard goes public as the Workshop: a page of silo.amontlabs.com, in the site's style, public from the start. The owner decides there; anyone signed in with GitHub can comment, vote and suggest features.
- 2026-11-16 · owner · GitHub is the Workshop's storage: issues and discussions of this repository, no database of ours and no separate backups. Agents post as a bot, not under the owner's name.
- 2026-11-16 · owner · Recordings are committed to the repository as files and played on the site.
- 2026-11-16 · owner · The home page stays the current landing page, trimmed (download off, navigation cleaned up). What Silo is belongs there, not in the Workshop.
- 2026-11-16 · owner · Pages for people must be obvious to use and short on text: one place to look, no explanatory paragraphs, no assistant-sounding copy.
- 2026-11-16 · owner · Silo restarts from an empty tree and is rebuilt piece by piece, carefully, one accepted entry of the feature list at a time. The old tree is frozen at tag `legacy-2026-11-16` (branch `legacy`). The restart and its state are in [RESTART.md](RESTART.md).
- 2026-11-16 · owner · The engine is written in Zig. It is the core piece: performant, secure, easy to reason about. The app, the command line and every tool only talk to it. Its design is data first, with events.
- 2026-11-16 · owner · The Mac app is written in Swift, comes first, and holds no logic. macOS computers are driven from Swift directly, never through Rust bindings.
- 2026-11-16 · owner · The engine also runs on Linux, to use a Linux machine as a host.
- 2026-11-16 · owner · Properties are stated and checked as we build: state properties, formal verification, whatever shows what always holds.
- 2026-11-16 · owner · The old code stays outside the new tree. It is read only when needed, through a fresh agent dedicated to that lookup, which brings back only what is relevant. Parts may be moved across, reviewed and cleaned up.
- 2026-11-16 · owner · Same GitHub repository: the old history stays on `legacy`, `main` starts fresh. Agents may push to this repository's remote.
- 2026-11-16 · owner · The feature list ([features.jsonl](features.jsonl)) is sorted so the most certain and most useful entries come first and every cut from the top is a product. The owner decides entry by entry on the dashboard's Stack; nothing is built without an accepted entry.
- 2026-11-16 · orchestrator, at the owner's word ("go as you think is cleanest") · Merging a task into main changes main only. It does not write the merged packages and files into the saved recipe; a recipe changes when the owner says so. Not built yet: today a merge still updates the recipe.
- 2026-11-16 · owner · The owner dashboard is a folder in this repository, not its own project.
- 2026-11-16 · owner · T3 Code runs on the Mac and connects into the computer; it does not run inside.
- 2026-11-16 · owner · A recipe sets up a whole working computer (agent, editor access, atelier, GUI automation), and making a recipe must be convenient.
- 2026-11-16 · owner · The third journey starts from a recipe or a checkpoint so a task has no setup to wait for. Updating main after a pull request merges is an ordinary pull, not a Silo feature.
- 2026-11-16 · owner · We use Silo for our own agent work only after the audit shows where the product is.
- 2026-11-15 · owner · The first user is the owner; a developer in a company fits the same product. Team features come later, on the same journeys.
- 2026-11-15 · owner · Three journeys come first: living in a computer, a computer per task, a task started from outside. Replaces "CI runner set aside" of 2026-11-10: its general form is the third journey.
- 2026-11-15 · owner · Features outside the three journeys stay and are reviewed one by one; nothing is frozen or removed without a review.
- 2026-11-15 · owner · Security is built properly and is not the current focus.
- 2026-11-15 · owner · Product truth is in `docs/product/`. Items for the owner go to the owner dashboard, not Linear. Linear keeps only business topics.
- 2026-11-15 · owner · A doc that no longer describes the code is deleted, not moved to an archive.
- 2026-11-15 · owner · No time estimates and no plans built from dates.
