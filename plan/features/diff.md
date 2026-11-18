## Proposed, not confirmed by the owner

### For the owner to settle

- Do agent instructions, disabled tests, changes that run later and changes of control become groups beside the plan's five, or flags on items inside them?
  - Recommendation: Flags, gathered at the top of the diff: nine groups would miss the old target of 15 lines for most tasks.
- Besides everything since the fork was made, which ranges does the first diff read: one agent session, between two checkpoints, the whole run across forks?
  - Recommendation: Between two checkpoints, which exist already; a diff per session needs to know which session wrote what.
- Does each fork get a risk level computed from its diff (size, groups touched, checks touched) and a flag when the diff is odd, or only the flagged changes?
  - Recommendation: Only the flagged changes, risky ones first: a low level reads as "safe", which the diff cannot promise.

### For whoever builds it

- Order and summary
  - Detail: The diff opens on a short summary by kind (code, tests, docs, packages, settings, files, databases) that can be judged in seconds, with the raw diff one step below.
  - Detail: In the diff and in its summary, changes that carry risk come before peripheral ones.
  - Detail: New folders and files, with their names and where they were placed, come before line-level changes.
  - Rule: A summary an agent wrote is labelled as the agent's account; the diff and the events beside it come from the engine and report what changed on disk.
  - Rule: The diff's summary states that it shows what changed and does not say whether the change is safe.
  - Rule: When a diff is too large to list, a stated budget chooses what is left out, and the diff says what was left out.
- What it calls out
  - Detail: The diff sets apart agent instructions and memory: rules files such as AGENTS.md, skills, plugins, hooks, memory files and databases, and the agent's own folder.
  - Rule: A change to agent instructions is marked as such and shown as plainly as code, including one the agent made to itself.
  - Property: No package install changes what an agent is instructed to do without that change appearing among the agent instructions in the diff.
  - Detail: The diff highlights changes that persist and run later: startup files, scheduled jobs, replaced programs and new programs on the path.
  - Test: A startup file or a scheduled job that stays inert until after a merge is planted in a fork; the diff shows it.
  - Detail: Tests disabled, skipped or removed, and a fork's changes to its own tests, checks or check configuration, are flagged together, with counts on the merge view.
  - Detail: A change of who controls something (a credential, a remote, a startup file, a rule) is its own event and is set apart in the diff.
  - Detail: The diff calls out destructive version-control actions: a conflict resolved by taking one side wholesale, a hard reset, rewritten history.
  - Detail: Unexpected new files are highlighted apart from edits, such as a second lockfile, new configuration at a project's top level, or agent files written into a source folder.
  - Detail: Agent context files (plans, memory, handoff notes) are set apart and can be marked ephemeral; ephemeral files are purged before a merge.
- Packages, databases and generated files
  - Detail: The package group lists every package and repository a fork added or changed, with version changes, including those pulled in indirectly.
  - Rule: Packages are read from what is installed in the fork, never from a lockfile.
  - Detail: Each package shows where it came from and the command that added it.
  - Detail: A fork's database schema changes are shown as one grouped change, however many migrations the agent wrote.
  - Detail: Build output, generated files and cache files are grouped apart from source changes by default and labelled as such.
  - Detail: The summary lists the commands, APIs, ports and schemas that differ between main and the fork.
  - Detail: Screenshots the agent took are shown in the diff beside the mockup or reference image.
- Reading it
  - Detail: Line changes show as word-level inline edits by default, whitespace ignored and syntax aware, with a split view as an option.
  - Detail: Each kind of change (code, packages, settings, files, databases) has one fixed, clearly visible colour, the same everywhere.
  - Rule: The diff view is for review only; editing goes through "open in my editor".
  - Detail: `silo diff` works per item and takes `--json` and `--stat`, which gives net lines added and removed per fork and per group.
  - Property: The diff view stays within a set, measured time budget on a change of more than 1,000 lines, 10 files and 20 comments.
- To check and find out
  - Test: A script run in one agent turn changes things through other processes and tools; the checkpoint and the diff show every change like a direct one.
  - To find out: Measure what each kind of diff and history costs to recompute, and store the slow kinds when they are produced; recompute-only was found clunky in another tool.
  - To find out: Read how VS Code or Theia build their diff and staging views, with their source cloned as context for the agent; Theia is not adopted as a base.

## Notes

- Old design: a manifest taken inside the computer at fork time and on the fork (allow-listed files, a shadow git of config folders, package lists, each repository's HEAD); the host compares them.
- Groups: Code, Packages, Settings and files, Other changed, Not kept, Ignored (a count). JSON and Markdown output; a `.siloignore` file.
- Also wanted: which package a change came from, changed git-ignored files such as `.env`, "database changed" from a schema hash, credential paths listed under Not kept.
- Command output must stream past the old exec limits (180 s, 768 KiB).
- Target: most real tasks read in 15 lines or fewer.

## From the old code

The old code had it partly built, about 6,400 lines, and it needs reworking.

Lessons:
- Silo's own files (endpoint.json, pulse files) must be ignored or every fork conflicts on them (M C2).
- Silo's own per-computer files (/etc/silo, receipts, machine ids) must be on the ignore list or every fork shows them as changes.
- Item ids are built only from what names the item, so the same id works in diff, conflict and choice.
- Idle macOS forks rewrite many preference and database files; they need a built-in ignore list.
- Databases are shown but not driven live.
- guest.exec is capped at 768 KiB and 180 s, so the collector runs detached in the guest and is read back in chunks.
- A known record with too few fields is an error, because a truncated stream would otherwise read as deleted files.
- The fork-time base is captured at the fork's first start, after the quiet step and before it reports ready.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/changes.rs`
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints/changes.rs`
- `app/SiloUI/src-tauri/engine/src/changes`
- `docs/SiloUI-CHANGES.md`
- `docs/direction/evidence/M.txt`
- `docs/SiloUI-CHANGES.md#buckets`
- `app/SiloUI/cli/src/ops.rs`
- `docs/direction/evidence/T5a.txt`
- `docs/product/JOURNEYS.md#L43`
- `app/SiloUI/src-tauri/engine/src/changes/diff.rs`
- `app/SiloUI/src-tauri/engine/src/changes/ignore.rs`
- `app/SiloUI/src-tauri/engine/src/changes/collect.rs`

Calls: `changes.diff`, `changes.capture`, `changes.baseline`, `silo diff`.
