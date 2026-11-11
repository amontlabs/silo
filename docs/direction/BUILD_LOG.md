# Direction build: log

Plan: [PLAN.md](PLAN.md). Agent rules: [AGENT-BRIEF.md](AGENT-BRIEF.md).

## Resume

- **Started:** 2026-11-10, from main at the commit that adds this file.
- **How work lands:** each item is a delegated agent in its own worktree and branch. It runs its
  own review loop with the `reviewer` model, then reports. The orchestrator reads the report,
  reviews boundary changes itself, merges the branch into main in the main checkout, regenerates
  the API files if two items touched them, and deletes the item's target folder
  (`<worktree>/app/SiloUI/target`, or `~/code/projects/silo-wt-targets/<item>` for items started
  before 2026-11-11). No pull requests; main is pushed now and then.
- **Build folders:** a local `post-checkout` hook in `.git/hooks` clones the main checkout's
  `app/SiloUI/target` into each new worktree (APFS clone, no space until files change). After
  merging, run `cargo build --manifest-path app/SiloUI/Cargo.toml --workspace --locked` in the main
  checkout so new worktrees start from current dependency builds.
- **Live checks** use the Dev engine, one item at a time. On 2026-11-10 a Dev app and engine from a
  deleted build (`silo-dev-target`) were still running; an item that needs the Dev engine
  replaces them with its own build.
- **Models:** Sonnet implements; Opus for T1, T2 and T5 (they define the security boundary);
  `reviewer` (Codex gpt-6.1-sol high) reviews.

## Status

| Item | State | Branch | Notes |
| --- | --- | --- | --- |
| S1 | started | | |
| T1 | started | | |
| T4 | started | | |
| T9 | started | | |

## Decisions taken while planning

These were not asked of the owner. Each is open to change.

1. **Fast-forward does not swap computers.** The direction first said "the fork becomes main".
   With the agent living in main, that would end its session and drop what main wrote since the
   fork (transcripts, history). Fast-forward now means "nothing can conflict, everything is
   applied". Swapping is the explicit Replace main. The direction was corrected.
2. **"Created by", not lineage, decides what a computer may touch.** A fork the owner made by hand
   can hold more access than its parent, so the parent's agent does not get it.
3. **The channel is a loopback port per computer plus a token**, not a new runtime patch, if
   spike S1 holds. The patch (gateway port to a Unix socket) is the fallback and a later hardening.
4. **The `silo` program goes inside the computer.** The direction said "nothing to install"; a
   program pushed by the engine at start gives agents the same command line and MCP server the
   device has, with no second implementation of the tools.
5. **A call that needs approval returns at once with an id** and the client waits by polling, so
   no connection is held open for minutes.
6. **Wire names stay** (`changes.keep`, `changes.promote`). Only what people and agents read is
   renamed. A wire rename can follow.
7. **Limits:** a safety floor on available memory and free disk for calls from computers, a
   warning before it, no counts. The owner asked for no hard caps.
8. **The macOS template keeps Claude Code** until its next revision, to avoid rebuilding every
   user's template now.
9. **Folder forks and `--pr` stay** as device-only commands. They belong to the old model but
   work without `.silo`; removing them is the owner's call.
10. **The hosted reviewer is hidden, not deleted.**
11. **First install of a macOS computer still uses Apple's network.** The disk is Apple's image and
    Silo's scripts; removing that window needs the bootstrap files inside the template.
12. **Not in this build:** the DNS query log, signing, the per-computer helper process for macOS,
    reflected-secret redaction, the `/.msb` review (the build removes Silo's use of the folder for
    requests, which narrows it).

## Found during the build
