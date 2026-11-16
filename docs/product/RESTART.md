# Restart: where we are

Silo is being rebuilt from an empty tree. A new session reads this file first. It says what is
decided, what is only proposed, where the work stands and what to do when a session starts. It is
deleted once the new tree exists; its content then lives in that tree's own rules.

## Decided

The owner's decisions are the entries dated 2026-11-16 in [DECISIONS.md](DECISIONS.md): an empty
tree, a Zig engine as the core, a Swift Mac app with no logic, the engine on Linux too, properties
checked as we build, the old code kept outside, the same GitHub repository, and the feature list
decided entry by entry.

## Proposed by the orchestrator, not confirmed by the owner yet

- The engine is state, calls and events. State is a set of records. A call asks for a change. An
  event says what happened. The app shows state and follows events.
- The core decides and touches nothing: it takes state and a call and returns the new state, events
  and things to do. Drivers do them (run a VM, set up the network, talk to GitHub).
- One name per concept everywhere: a section of the list, a group of calls, one engine file, a
  command, a screen.
- Each concept states its properties, and a simulator checks them on every change with random
  sequences of calls, crashes included.
- The engine is its own program, not a library inside the app, so computers keep running when the
  app closes.
- Layout: `product/` (the list, decisions, properties), `engine/` (model, core, drivers, server,
  sim), `cli/`, `app/`, `guest/`, `tools/`.
- Zig pinned to one version (0.17.1 was the latest stable on 2026-11-02), with its docs at hand.
- The new tree is checked out in a new folder beside this one; this folder stays the old tree.

## Where the work stands

1. Done: the old tree is frozen (tag `legacy-2026-11-16`, branch `legacy`, both on GitHub).
2. Done: the feature list, 119 entries in 13 stages and 16 areas ([features.jsonl](features.jsonl),
   [stages.json](stages.json), [areas.json](areas.json)), and the Stack view on the dashboard.
3. Now: the owner goes down the Stack and gives verdicts. The first 41 entries (four stages) are
   the starter.
4. Now: taking the Workshop live. It is built and merged on local `main` (2026-11-16, merge
   1808ac06c): the home page ("Computers your agents can fork."), the Workshop as a map of the
   product's 16 areas (tiles under 1200 px), the server, and `tools/workshop` (`setup`, `store`,
   `doctor`, `sync`, `pull`, `ask`, `comment`, `building`, `built`), all described in
   [docs/WORKSHOP.md](../WORKSHOP.md). The areas are in [areas.json](areas.json) and each entry's
   `area` in [features.jsonl](features.jsonl). It was run end to end only against the stand-in for
   GitHub (`node website/scripts/workshop-dev.mjs --fake`, with `--progress` for a later state).
   Nothing has met the real GitHub or a real deployment yet. What is left, in order:
   - `main` is NOT pushed since the first Workshop merge: a push to `main` deploys the public site.
     The remote branch `workshop-preview` holds an older state.
   - The owner runs `tools/workshop/bin/workshop setup` (creates the public GitHub App "Silo
     Workshop", two clicks), installs it on this repository only, and checks that expiring user
     tokens are on.
   - The owner sets the Vercel project: the seven variables from
     `~/.local/state/silo-workshop/vercel.env`, the build command
     `npm --prefix website run build:vercel`, the install command `npm --prefix website ci`.
   - Go-live then follows [PLAN.md](PLAN.md), section 12, not the old `sync` then `pull` routine.
5. Now: the plan as data. The owner asked (2026-11-16) for one source of truth for the plan, with
   one place to edit and everything else generated. The design is [PLAN.md](PLAN.md): what a
   feature is lives in `plan/` (one file per feature), what happened to it lives on its GitHub
   issue, everything shown is computed by one shared module, and agents use one `plan` command.
   It was reviewed twice; [PLAN-RESEARCH.md](PLAN-RESEARCH.md) records the existing tools examined
   and why none is adopted. Nothing of it is built. Next: its build items 0 to 10 (section 14), in
   order, through the merge gate. Five points in it wait for the owner (listed at its top); it
   proceeds on the recommended answers until the owner says otherwise. Until its cutover (item 9) the
   files in this folder stay the plan, and the routine below still applies.
6. Then: start `main` again as the new tree, carrying the website, the Workshop, this folder and the
   rules. Wait for the owner to confirm the proposals above first.
7. Then: design the shell's records, calls and events, and show it on the dashboard for approval
   before any code.
8. Then: build the taken entries in list order, one at a time, each proven from the command line
   before it gets a screen.

## When a session starts

1. Read this file and the 2026-11-16 entries of [DECISIONS.md](DECISIONS.md).
2. Run `tools/dashboard/bin/dashboard answers` and `tools/dashboard/bin/dashboard verdicts`. They
   print what the owner answered and decided on the dashboard since the last session.
3. Write each verdict into the `verdict` field of its line in [features.jsonl](features.jsonl), and
   a note that decides something into [DECISIONS.md](DECISIONS.md). Then acknowledge them
   (`dashboard ack …`, `dashboard verdicts ack <id>@<row>`).
4. Check the dashboard is running at http://127.0.0.1:4517/ and start it if not
   (`tools/dashboard/bin/dashboard serve`).
5. Continue from "Where the work stands", and update that section when a step is done.

## Rules for the restart

- Add nothing to the old tree except the dashboard and this folder.
- Do not read the old code while working on the new tree. When something from it is needed, start
  a fresh agent for that one lookup, give it the entry's line in
  [old-code-map.jsonl](old-code-map.jsonl) (the files, calls and lessons worth reading), and take
  back only what is relevant.
- The journeys document of the old app is gone: the list replaces it.
- The raw material behind the list (six sweeps, 427 entries, the script that merged them) is on the
  owner's Mac in `~/.local/state/silo-dashboard/sweep/`.
