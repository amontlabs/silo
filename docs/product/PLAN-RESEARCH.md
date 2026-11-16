# Plan as data: research

Read on 2026-11-16 from the repositories themselves (GitHub API for license, latest release and last push; README, docs and issue trackers for the rest). "Open items" is GitHub's count of open issues plus pull requests. Quotes are copied from the linked source. Anything not opened today is listed in section 5.

Fit is judged against the brief's eight points, by number: **1** one edit place per fact, the rest generated, a failing check; **2** structured fields plus free text that grows; **3** a cold agent finds, reads and edits in one step, with errors naming file, field and fix; **4** owner decides on the page and in conversation, public votes and comments on issues, agents mark building and built; **5** parallel worktrees, one gate, no conflict magnet; **6** public repository; **7** small, Node 26 / Python 3.11+, product in Zig and Swift; **8** schema change is routine.

## 0. The baseline, measured in the checkout

These are the drift surfaces every option below is judged against.

- `docs/product/features.jsonl` holds one feature per line, 119 lines. Each line carries its own position (`"n": 5`) as well as its place in the file. Git's unit of conflict is therefore the whole feature, and a reorder renumbers every line after it.
- Adding one field touched almost the whole file: `site/workshop-map` adds `area` and changes 110 of the 119 lines (`git diff --stat main site/workshop-map`: 110 insertions, 110 deletions in `features.jsonl`). Any other branch that edits any of those features conflicts with it.
- The verdict has two homes. New verdicts are GitHub labels; the `verdict` field keeps "carried-over" ones; `workshop pull --write` copies label to field, and "a verdict cleared on GitHub is never cleared from the file" (`tools/workshop/README.md`).
- Two tool-written values sit inside hand-edited text: `sync` writes `"issue": N` into feature lines and appends ` (#N)` to items of `DECISIONS.md`.
- A decision has no id. `sync` matches a decision to its issue "only when … its body is exactly that text", so fixing a typo changes its identity. `DECISIONS.md` is newest-first, so two branches that each add a decision insert at the same line.
- The verdict vocabulary `take, refine, later, drop` is written out in at least five places: `tools/dashboard/src/features.js:12`, `tools/dashboard/src/store.js:57` (an SQL `CHECK`), `tools/dashboard/src/views.js:385`, `tools/workshop/src/names.js:9`, `website/src/workshop/data.ts:11`. Three programs read `features.jsonl`, each its own way: the site refuses a bad line with file and line (`website/src/workshop/data.ts`), the dashboard skips it silently (`tools/dashboard/src/features.js`: "null when the line cannot be used"), the tool does not check fields (`tools/workshop/src/jsonl.js`).
- Prose repeats facts the data already holds: "119 entries in 13 stages", "the first 41 entries (four stages)" (`RESTART.md`), "Four stages" (`PRODUCT.md`).
- "Being built" is a GitHub label (branch `workshop/building`, not merged) and "built" is a closed issue (`workshop built N`). Neither is in the repository, so neither changes in the same merge as the code.
- Free text has no room: `what` and `why` are one JSON string each.
- What already works and matches the good tools: errors with file and line (`features.jsonl:3: missing "name"`), a one-way `sync` that is idempotent and finds issues by a marker (`<!-- silo-entry: id -->`), `--json` on every command, the site built from the files at build time (`virtual:workshop-data`), nothing generated is committed.
- Toolchain check run here: Node v26.10.0 has `node:sqlite` built in and no built-in YAML or TOML parser; Python 3.12 has `tomllib` (read only) and no YAML parser. JSON is the only format every toolchain in the brief reads from its standard library (Zig `std.json` and Swift `JSONDecoder` from knowledge, not run).

## 1. Options examined

### 1.1 Trackers built for coding agents

| Option | What it is | Where the truth lives | License | Latest release | Maintenance | How an agent uses it | GitHub sync | Fit |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| [Beads `bd`](https://github.com/gastownhall/beads) (Steve Yegge; moved from `steveyegge/beads`) | Dependency-graph issue tracker for agents | A Dolt SQL database in `.beads/embeddeddolt/`, pushed to `refs/dolt/data` on the git remote. "`.beads/issues.jsonl` is an export for viewers and interchange, not the source of truth or a backup" | MIT | v1.3.4, 2026-11-04 (v1.4.0-rc.2, 2026-11-13) | Very active, about one release a week; 28.8k stars; 1,806 open items | `bd prime` (context), `bd ready --json`, `bd show <id> --json`, `bd update <id> --claim`, `bd dep add`; hooks via `bd setup claude`; MCP server `beads-mcp` | `bd github sync / pull / push`, both ways; two-way tracker sync is "lossy by design" ([#1573](https://github.com/gastownhall/beads/issues/1573)) | Not for the plan. Meets 3 and the claim half of 5. Fails 1 (truth is outside the reviewed git diff; the site would read an export), 4 (no verdict; its sync would compete with the Workshop), 7 (Go binary, embedded Dolt, migrations), 8 (its schema, its migrations). 2 partly (fixed issue fields, a free `metadata` JSON, description and notes) |
| [Backlog.md](https://github.com/MrLesk/Backlog.md) | Task board whose tasks are Markdown files in the repo | One `.md` per task in `backlog/tasks/`, YAML front matter; also `decisions/`, `docs/`, `milestones/`, `drafts/`, `archive/`, `config.yml` | MIT | v1.55.3, 2026-10-23 | Active, several releases a month; 7.4k stars; 107 open items; built by agents using itself | CLI first (`backlog instructions overview`, `backlog task view/list/edit --json`); MCP optional ("CLI instructions are the default AI setup") | None. Declined: "a major integration and source-of-truth design problem" ([#231](https://github.com/MrLesk/Backlog.md/issues/231)) | Closest in shape, wrong entity. Meets 1 and 3. 2 partly (task schema; unknown keys kept since [#918](https://github.com/MrLesk/Backlog.md/issues/918), not validated). Fails 4 and 8 (fixed statuses and fields). 5 partly: one file per item, but sequential ids collide across clones and stay that way ([#711](https://github.com/MrLesk/Backlog.md/issues/711)) |
| [Task Master](https://github.com/eyaltoledano/claude-task-master) | PRD-to-tasks manager | One `.taskmaster/tasks/tasks.json`, plus per-task text files regenerated from it | MIT with Commons Clause: not OSI-approved | task-master-ai@0.43.1, 2026-03-31 | Last commit 2026-04-23; docs moved to another product's site | CLI and MCP | None in its docs folder | Excluded by license. Also a single shared JSON file and a manual regenerate step ("After any updates to tasks.json, regenerate the task files to keep them in sync") |
| [Seeds `sd`](https://github.com/jayminwest/seeds) | Beads-like tracker without a database | `.seeds/issues.jsonl`: "The JSONL file IS the database" | MIT | v0.7.0, 2026-11-05 | One maintainer, 131 stars, needs Bun | `sd prime`, `sd ready`, `--json` everywhere | None seen in the README | Too small to depend on. Worth reading for two mechanisms: a git merge driver for its JSONL (`sd merge-driver`) and a hook that denies hand edits to it (`sd guard`) |
| [ticket `tk`](https://github.com/wedow/ticket) | Beads replacement in one bash script | Markdown with YAML front matter in `.tickets/`, file name = id | MIT | v0.3.2, 2026-02-03 (pushed 2026-11-09) | 932 stars | CLI, `tk query` through `jq` | None | Shape matches, too thin (no schema, no validation) |
| [beans](https://github.com/hmans/beans) | Flat-file tracker with a GraphQL query engine | Markdown in `.beans/` | Apache-2.0 | v0.4.2, 2026-03-10 | No push since 2026-04-06 | `beans prime`, GraphQL queries | None | Stale |
| [beads_rust `br`](https://github.com/Dicklesworthstone/beads_rust) | Rust port of the old Beads design | SQLite with a JSONL export | "MIT License (with OpenAI/Anthropic Rider)": not a plain OSI license | v0.7.6, 2026-10-15 | Active | CLI | None seen | Excluded by license |
| [git-bug](https://github.com/git-bug/git-bug) | Bug tracker stored in git objects | Git refs, not files | GPL-3.0 | v0.12.0, 2026-11-07 | Active, 10.9k stars | CLI | Bridges import and export GitHub | Truth is not a file: no `cat`, no diff in review, no build-time read. Fails 1, 3 |
| [CCPM](https://github.com/automazeio/ccpm) | Skills that turn a PRD into epics, GitHub issues and worktrees | Its README says both "CCPM uses GitHub Issues as the source of truth" and "Files are the source of truth" | MIT | No releases; pushed 2026-11-04 | 8.5k stars | Skills and bash scripts | Files to issues, then renames local files to issue numbers | A process kit, not a data model; shows how easily the truth gets stated twice |

Also seen, not examined in depth: [sudocode](https://github.com/sudocode-ai/sudocode) (Apache-2.0, last release 2026-03-18), [chainlink](https://github.com/dollspace-gay/chainlink) (MIT, SQLite, 2026-03-20), [vibe-kanban](https://github.com/BloopAI/vibe-kanban) (Apache-2.0, an orchestrator, not a plan store).

### 1.2 Spec-driven development kits

| Option | What it is | Where the truth lives | License | Latest release | Maintenance | How an agent uses it | GitHub sync | Fit |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| [GitHub Spec Kit](https://github.com/github/spec-kit) | Process kit: constitution, specify, plan, tasks, implement, converge | One folder per feature (`specs/NNN-name/spec.md`, `plan.md`, `tasks.md`), Markdown only; ids like `FR-001`; open points as `[NEEDS CLARIFICATION: …]` | MIT | v1.4.1, 2026-11-16 | Very active (five releases in two weeks); 150k stars; 250 open items | Skills run in the agent chat; `specify` CLI | One way: tasks to issues (bundled `github` extension, [#4421](https://github.com/github/spec-kit/issues/4421)) | Not a registry. Fails 1 (a feature's spec goes stale when a later feature changes the same behaviour, [#620](https://github.com/github/spec-kit/issues/620)), 4, 7 (large and fast-moving). 5 partly: numbered prefixes collide across branches ([#1066](https://github.com/github/spec-kit/issues/1066)) and dense ids force renumbering ([#4065](https://github.com/github/spec-kit/issues/4065), open) |
| [OpenSpec](https://github.com/Fission-AI/OpenSpec) | Living specs plus change folders | `openspec/specs/<capability>/spec.md` is the current truth; a change is a folder (`proposal.md`, `design.md`, `tasks.md`, delta specs, `.openspec.yaml`) merged into the specs on archive | MIT | v2.0.0, 2026-11-11 | Very active; 75k stars; 214 open items; 2.0 removed its slash commands five days ago | Skills plus a CLI with `--json` (`list`, `show`, `validate --strict`, `status`, `instructions`) and a written [agent contract](https://github.com/Fission-AI/OpenSpec/blob/main/docs/agent-contract.md) | None in the CLI reference | Best fit for the behaviour text, not for the list. 3 and 8 yes (custom schemas, `x-*` keys). 2 partly: strong prose grammar, no per-feature data fields, no order, no dependencies between specs. 4 no. 5 partly: change folders do not collide, but archiving two changes to one requirement dropped scenarios silently ([#1246](https://github.com/Fission-AI/OpenSpec/issues/1246)) |
| [Kiro specs](https://kiro.dev/docs/specs/) | Specs inside AWS's IDE and CLI | `requirements.md` (or `bugfix.md`), `design.md`, `tasks.md` per spec | Proprietary (the public repo has no license) | n/a | Vendor product | Inside Kiro only | n/a | Excluded: not open source. Its [correctness properties](https://kiro.dev/docs/specs/correctness/) are the one idea to keep |
| [BMAD Method](https://github.com/bmad-code-org/BMAD-METHOD) | Agent personas and document workflow | PRD, architecture and story Markdown files | MIT plus a trademark notice | v6.12.4, 2026-10-14 | Active, 55k stars | Agent commands | Not examined | A process, not a store. Not examined in depth |

Also seen: [Agent OS](https://github.com/buildermethods/agent-os) (MIT, v3.0.0, 2026-01-20), [cc-sdd](https://github.com/gotalab/cc-sdd) (MIT, v3.1.0, 2026-09-23), [spec-workflow-mcp](https://github.com/Pimzino/spec-workflow-mcp) (GPL-3.0, no releases).

### 1.3 GitHub's own features as the store

| Feature | What it gives | API | Limits found | Fit as the truth |
| --- | --- | --- | --- | --- |
| [Issue types](https://docs.github.com/en/issues/tracking-your-work-with-issues/configuring-issues/managing-issue-types-in-an-organization) | A type per issue (default task, bug, feature), for organizations and user accounts | Not stated on the page | "up to 25 issue types" | Could label mirrored entries as Feature; replaces the `workshop:entry` label, nothing more |
| [Issue fields](https://github.blog/changelog/2026-07-02-issue-fields-are-now-generally-available) (GA 2026-07-02) | Typed fields on issues, defined per organization under Settings, Planning; defaults Priority, Effort, Start date, Target date; visibility controls for non-members | [REST](https://docs.github.com/en/rest/issues/issue-field-values): `GET/POST/PUT …/issues/{n}/issue-field-values`, `DELETE …/{issue_field_id}`, version 2026-03-10 | "Only users with push access to the repository can add issue field values" | Schema is an organization setting changed by clicks: it applies to every repository of `amontlabs`, is not reviewed, not versioned with the code. Fails 8 |
| [Sub-issues](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/adding-sub-issues) | Parent and child issues with progress | [REST](https://docs.github.com/en/rest/issues/sub-issues): list, add, remove, reprioritize | "up to 100 sub-issues per parent issue", "up to eight levels" | Would let a feature's properties or tasks be children; ordering is by before/after moves, not a number |
| [Issue dependencies](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/creating-issue-dependencies) | Blocked by / blocking, with a Blocked icon in lists and projects | [REST](https://docs.github.com/en/rest/issues/issue-dependencies): `GET/POST/DELETE …/dependencies/blocked_by`, `GET …/dependencies/blocking`; `gh issue create --blocked-by` | None stated | A good target for a one-way mirror of "needs" |
| [Projects](https://docs.github.com/en/issues/planning-and-tracking-with-projects/learning-about-projects/about-projects) | Views with custom fields | GraphQL and now [REST](https://docs.github.com/en/rest/projects/items) (items, field values) | "up to 50 fields"; a project belongs to a user or an organization, not a repository | A second place to keep in step; no gain over issue fields here |
| [Issue forms](https://docs.github.com/en/communities/using-templates-to-encourage-useful-issues-and-pull-requests/syntax-for-issue-forms) | Typed input for a new issue; can set labels, type, projects | n/a | "currently in public preview"; answers are "converted to markdown and added to the body" | Useful for suggestions; the answers are not readable as fields afterwards |

As the store for the whole plan: 4 is met completely and 5 has no merge conflicts, because nothing is in git. It fails 1 in the sense that matters here (GitHub accepts any write, so no check can refuse one), 3 (a cold agent needs the network and a token; a feature cannot change in the same commit as the code that builds it), 5's gate (a change is live before anything is merged) and 8. Section 2.5 says what "truth" and "mirror" mean per field.

### 1.4 Docs-as-data formats and tools

| Option | What it is | License | Latest release | Fit |
| --- | --- | --- | --- | --- |
| One file per item + [JSON Schema](https://json-schema.org/), checked with [ajv](https://github.com/ajv-validator/ajv) (Node) or [check-jsonschema](https://github.com/python-jsonschema/check-jsonschema) (Python) | A schema that is itself data; both toolchains can read it; editors complete from `$schema` | ajv MIT; check-jsonschema Apache-2.0 | ajv v8.20.0, 2026-04-24; check-jsonschema 0.39.0, 2026-10-26 | Covers the shape of one file. Not references between files, cycles, order or "the recording exists": those stay custom checks in every tool looked at. Its errors give a path and a message, not a line or a fix |
| [Astro content collections](https://docs.astro.build/en/guides/content-collections/) (Astro 7) | Zod schema per collection; `glob()` for one file per entry, `file()` for many entries in one file; emits a JSON Schema per collection into `.astro/collections/` | MIT | Astro 7 | The site is Vite and React, not Astro; adopting it means changing the site's framework, and the command-line tools would still parse the files themselves |
| [content-collections](https://github.com/sdorra/content-collections) | The same idea with a Vite adapter and a CLI | MIT | @content-collections/vite 0.3.1, 2026-09-21 | Fits the site only; a second reader next to the tool's reader is the drift this work is meant to remove |
| [Velite](https://github.com/zce/velite), [Contentlayer](https://github.com/contentlayerdev/contentlayer) | Same family | MIT | Velite v0.4.0, 2026-06-17; Contentlayer v0.3.4, 2023-06-29 (unmaintained) | No reason to prefer over the above |
| [CUE](https://github.com/cue-lang/cue) | Data, schema and constraints in one language | Apache-2.0 | v0.17.1, 2026-07-16 | Can express cross-file constraints, but adds a Go binary and a language agents know less well. Fails 7 |
| [KDL](https://github.com/kdl-org/kdl) | A document language | Not checked | spec 2.0.0, 2024-12-22 | No standard-library parser in any of the four toolchains |
| YAML or TOML front matter | The usual Markdown-with-metadata form | n/a | n/a | Needs a parser dependency in Node (both formats) and in Python (YAML); see section 0 |

### 1.5 Decision logs

| Option | What it is | License | Latest release | Fit |
| --- | --- | --- | --- | --- |
| [MADR](https://github.com/adr/madr) | A template: one Markdown file per decision, front matter `status` (`… superseded by ADR-0123`), `date`, `decision-makers` | MIT or CC0-1.0 | 4.0.0, 2024-09-17 (repo pushed 2026-11-13) | A format, nothing to install. Its long body is more than a one-line decision needs; the front matter and "superseded by" are the part to keep |
| [adr-tools](https://github.com/npryce/adr-tools) | Shell scripts that number and link ADR files | GPL | 3.0.0, 2018-07-25 | Unmaintained; sequential numbers collide across branches |
| [log4brains](https://github.com/thomvaill/log4brains) | ADR site generator | Apache-2.0 | v1.1.0, 2024-12-17 | No release in two years |
| [changesets](https://github.com/changesets/changesets) (already used in `app/SiloUI/.changeset/`), [towncrier](https://github.com/twisted/towncrier) | One small file per change, compiled into the changelog | MIT | @changesets/cli 3.0.3, 2026-09-14; towncrier 26.9.0, 2026-09-04 | The precedent for any append-only log: towncrier exists so that developers do not have "one single file which developers all write to and produce merge conflicts" |

### 1.6 Requirement-to-test traceability

| Option | What it is | License | Latest release | Fit |
| --- | --- | --- | --- | --- |
| [Doorstop](https://github.com/doorstop-dev/doorstop) | One YAML file per requirement; links to parents carry the parent's fingerprint | LGPL-3.0 | v3.2, 2026-07-10 | Python, so installable. Its document tree and levels are more than needed and its files would be a second registry. The fingerprint on a link is the idea to take |
| [OpenFastTrace](https://github.com/itsallcode/openfasttrace) | Ids with a revision (`req~name~1`), `Covers:` and `Needs:` in specs, tags in code comments (`// [impl->dsn~validate-authentication-request~1]`), a tracer that reports what is uncovered or outdated | GPL-3.0 | 4.11.0, 2026-10-18 | Language-neutral tags suit Zig and Swift; it needs a JVM, which is outside the toolchain |
| [Sphinx-needs](https://github.com/useblocks/sphinx-needs) with sphinx-codelinks and sphinx-test-reports | Needs as Sphinx directives, links to code and test results | MIT | sphinx-needs 9.2.0, 2026-11-04 | Brings the whole Sphinx build |
| [StrictDoc](https://github.com/strictdoc-project/strictdoc) | Requirements in `.sdoc` files, `@relation(…)` markers in source | Apache-2.0 | 0.30.4, 2026-10-22 | Its own document format; a second registry |
| [Reqflow](https://github.com/goeb/reqflow) | Traces requirement ids across documents | GPL | v1.6.0, 2019-03-11 | Stale |

### 1.7 How open projects keep a plan in the repository

| Project | Where the truth lives | What is generated | What is checked | Source |
| --- | --- | --- | --- | --- |
| web-features (its `features/` folder lists at least 1,000 files, authored and generated together) | `features/<id>.yml`, authored | `features/<id>.yml.dist`, committed, header "Do not edit this file by hand" | `npm run test:dist` runs the generator with `--check`; JSON Schema; types generated from the schema | [CONTRIBUTING](https://github.com/web-platform-dx/web-features/blob/main/docs/CONTRIBUTING.md), [package.json](https://github.com/web-platform-dx/web-features/blob/main/package.json) |
| Kubernetes KEPs | A folder per KEP: `README.md` (prose) and `kep.yaml` (status, stage, milestones, `see-also`, `replaces`) | A website | The folder name starts with the tracking issue's number, and "the issue where the current state of the KEP is being updated" is the tracker, so status exists in two places | [kep.yaml template](https://github.com/kubernetes/enhancements/blob/master/keps/NNNN-kep-template/kep.yaml), [keps/README](https://github.com/kubernetes/enhancements/blob/master/keps/README.md) |
| Python PEPs | Headers in each PEP file | "(PEP 0) is automatically generated based on the metadata headers in other PEPs" | Lint in pre-commit and CI | [python/peps README](https://github.com/python/peps) |
| Rust feature gates | Declarations in the source, each naming its tracking issue | The Unstable Book | `tidy` fails with `{file}:{line}: no tracking issue for feature {name}`, requires a test proving each unstable feature is gated, and requires the list sorted | [tidy/features.rs](https://github.com/rust-lang/rust/blob/master/src/tools/tidy/src/features.rs) |
| Go release notes | One fragment per change in `doc/next`, named after the proposal's issue number | The notes, "concatenated in sorted order by pathname" | "Files in this repo's `api/next` directory must have corresponding files in `doc/next`" | [doc/README.md](https://github.com/golang/go/blob/master/doc/README.md) |
| TC39 proposals | Hand-edited tables in a README | `proposals.json`, in a separate repository | Nothing checks the tables | [tc39/dataset](https://github.com/tc39/dataset) |

## 2. The most relevant ones

### 2.1 Backlog.md

**How it works.** `backlog init` creates `backlog/` with `tasks/`, `drafts/`, `decisions/`, `docs/`, `milestones/`, `archive/` and `config.yml`. A task is one file, `backlog/tasks/back-800 - Describe-accurately-what-a-duplicate-task-ID-blocks.md`, copied from its own repository and trimmed:

```markdown
---
id: BACK-800
title: Describe accurately what a duplicate task ID blocks
status: Done
assignee:
  - '@claude'
created_date: '2026-11-10T07:23:00Z'
labels: []
dependencies: []
references:
  - 'https://github.com/MrLesk/Backlog.md/issues/1146'
  - src/utils/duplicate-detection.ts
priority: low
type: bug
ordinal: 406000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
When two task files claim one ID, the warning says: …
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 With a duplicate task ID in the store, the warning says what actually happens: …
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Reproduce in a scratch project …
<!-- SECTION:PLAN:END -->
```

Agents are told to start with `backlog instructions overview` and to use commands, not hand edits ("so field types and metadata stay consistent"). Read commands give "stable, versioned JSON". What is ready is computed, not stored: the task view carries a `dependencyGraph` "derived from the whole visible corpus at read time and never stored in the Markdown file", and `readiness` lists `blockingDependencies` and `missingDependencies`; "both lists fail closed: an unknown or ambiguous dependency blocks instead of being treated as satisfied" ([CLI-INSTRUCTIONS.md](https://github.com/MrLesk/Backlog.md/blob/main/CLI-INSTRUCTIONS.md)). For branches it scans the task files of other active branches (`check_active_branches`, `active_branch_days`).

**What it gets right for Silo.** One file per item; data and growing prose in one place; sections fenced by markers so a command can replace one section and leave the rest; order as a sparse number (`ordinal: 406000`) so moving one item rewrites one file; derived state never written; decisions and documents as sibling folders with the same commands; CLI before MCP.

**What would not fit.** The entity is a task with a status column, not a feature with a verdict, an area, a stage, properties and recordings. Custom keys survive an edit only since [#918](https://github.com/MrLesk/Backlog.md/issues/918) (closed 2026-10-19) and are neither validated nor queryable; saving the config still drops unknown keys ([#1172](https://github.com/MrLesk/Backlog.md/issues/1172), open). Ids are `prefix-number` and collide when two clones create a task; the maintainer keeps it so: "We are keeping stable, incremental numeric task IDs … A random or collision-free ID mode is not planned" ([#711](https://github.com/MrLesk/Backlog.md/issues/711)). No GitHub mirror, by decision ([#231](https://github.com/MrLesk/Backlog.md/issues/231)). Worktree bugs have been real: the MCP server wrote to the main checkout instead of the worktree ([#558](https://github.com/MrLesk/Backlog.md/issues/558)), changes in worktrees were not picked up ([#689](https://github.com/MrLesk/Backlog.md/issues/689)). Claims in git are still an open proposal from a fork ([#1063](https://github.com/MrLesk/Backlog.md/issues/1063)). Adopting it would still leave verdicts, the mirror, properties and the site to a second tool editing the same files.

**To borrow.** The file shape, the section markers, derived readiness that fails closed, `instructions overview` as the entry, the archive that keeps an id forever.

### 2.2 OpenSpec

**How it works.** Two trees. `openspec/specs/<capability>/spec.md` says what the system does now. `openspec/changes/<name>/` holds a proposed change: `proposal.md`, `design.md`, `tasks.md`, delta specs, and an optional `.openspec.yaml` ("schema, created, skip_specs, retire_capabilities, plus your own x-* keys"). Archiving a change merges its deltas into the specs and moves the folder to `changes/archive/`. A spec from its own repository (`openspec/specs/cli-validate/spec.md`):

```markdown
# cli-validate Specification

## Purpose
Define `openspec validate` behavior for validating changes and specs with actionable remediation guidance and structured output.

## Requirements

### Requirement: Validation SHALL provide actionable remediation steps
Validation output SHALL include specific guidance to fix each error, including expected structure, example headers, and suggested commands to verify fixes.

#### Scenario: No deltas found in change
- **WHEN** validating a change with zero parsed deltas
- **THEN** show error "No deltas found" with guidance: …
```

and a delta (`## ADDED Requirements`, `## MODIFIED Requirements`, `## REMOVED Requirements`). `openspec validate --all --strict --json` checks structure. The [agent contract](https://github.com/Fission-AI/OpenSpec/blob/main/docs/agent-contract.md) fixes the machine surface: "One JSON document per invocation", prose on stderr, and one diagnostic shape:

```json
{ "severity": "error" | "warning" | "info", "code": "snake_case_string", "message": "human sentence", "target": "dotted.surface (optional)", "fix": "one actionable sentence/command (optional)" }
```

**What it gets right for Silo.** A grammar for behaviour that a test can prove (a requirement with SHALL, scenarios with WHEN and THEN), with the requirement's name as its stable id. A spec is "a behavior contract, not an implementation plan". Change folders let several agents propose at once. The schema of the artifacts is itself a file that can be forked. The error shape is exactly point 3.

**What would not fit.** No data fields per capability (no order, stage, area, needs, verdict) and no dependencies between specs. Specs describe what is built; most of Silo's list is not built and awaits a verdict. The merge of deltas into specs is a generated step that lost data: "the second archive overwrites the first and silently drops scenarios … There is no warning, diff, or conflict indicator" ([plan in its repository](https://github.com/Fission-AI/OpenSpec/blob/main/openspec-parallel-merge-plan.md), [#1246](https://github.com/Fission-AI/OpenSpec/issues/1246) closed, [#1387](https://github.com/Fission-AI/OpenSpec/issues/1387) open). Version 2.0, five days ago, changed where skills live and removed commands; a major version that recent is a reason to wait.

**To borrow.** The requirement and scenario grammar for a feature's properties; the diagnostic shape; one JSON document per call; the split between commands for people and commands for agents; "optional keys are omitted, not null".

### 2.3 Beads

**How it works.** `bd init` creates `.beads/` with an embedded Dolt database. Agents never read files: they run `bd prime`, `bd ready`, `bd show <id> --json`, `bd update <id> --claim`, `bd close <id>`. Dependencies are typed; four types block (`blocks`, `parent-child`, `conditional-blocks`, `waits-for`), six only annotate (`related`, `tracks`, `discovered-from`, `caused-by`, `validates`, `supersedes`). "An issue is ready when ALL of its blocking dependencies are closed." Ids are hashes (`bd-a1b2`, children `bd-a3f8.1`) so two agents never mint the same one. All worktrees of a repository "use the same beads workspace". Sync is `bd dolt push / pull` to `refs/dolt/data`, apart from the code's branches. One exported issue from its repository (trimmed, owner removed):

```json
{"id":"bd-main-idj","title":"Pattern-collapse pass: mechanical cruft inventory and reduction","description":"Quantify near-duplicate functions, dead code, … stage but do not merge.","status":"in_progress","priority":2,"issue_type":"chore","owner":"…","created_at":"2026-04-18T16:19:12Z","started_at":"2026-04-18T16:30:16Z","dependency_count":0,"dependent_count":0,"comment_count":0}
```

JSON output is moving to an envelope, `{"schema_version": 1, "data": <payload>}` ([json-schema.md](https://github.com/gastownhall/beads/blob/main/docs/reference/json-schema.md)).

**What it gets right for Silo.** The agent side: one command to get context, one to ask "what is ready", an atomic claim, typed links, ids that need no coordination, a versioned JSON contract, docs that name the code they were checked against ("Freshness source: `cmd/bd/output.go` …").

**What would not fit.** Its history is the warning. The first design kept JSONL in git with SQLite as a cache; users reported "a LOT of trouble with sync issues, concurrency, and corruption of the json store" and the maintainer answered that "our 3-way merging had a lot of really awful bugs until version v0.24.1" ([#158](https://github.com/gastownhall/beads/issues/158)); exports were overwritten by imports ([#1623](https://github.com/gastownhall/beads/issues/1623)). The move to Dolt made the database the truth and produced "Tasks disappear … the db is unreachable" ([#2573](https://github.com/gastownhall/beads/issues/2573); the maintainer names the loop "server down → commands fail → init refused → nuke .beads/ → data lost"), three checkouts with three different issue counts ([#3135](https://github.com/gastownhall/beads/issues/3135)), a migration that made pulls unmergeable ([#4259](https://github.com/gastownhall/beads/issues/4259)), stale reads across sessions ([#4746](https://github.com/gastownhall/beads/issues/4746), open) and a ref that `git push --mirror` destroys ([#5266](https://github.com/gastownhall/beads/issues/5266), open). For the plan specifically: a bead's change is not part of a branch's diff, so it passes no gate and no review; the site would build from an export; a schema migration needs "exactly one designated clone" to run it. 1,806 open items is a lot of surface for a 120-feature list.

**To borrow.** `prime`, `ready`, `show --json`, the envelope with a schema version, typed dependencies (at least "needs" versus "related"), and the rule in its AGENTS.md snippet: "Do not use markdown TODO lists for task tracking."

### 2.4 web-features (the pattern, not a tool)

**How it works.** Several hundred features, one authored file each, id = file name. `features/grid.yml` (trimmed):

```yaml
name: Grid
description: CSS grid is a two-dimensional layout system, which lays content out in rows and columns.
spec: https://drafts.csswg.org/css-grid-3/
group: grid
caniuse: css-grid
status:
  compute_from: css.properties.grid
compat_features:
  - css.properties.display.grid
```

Beside it, `features/grid.yml.dist`, written only by `npm run dist` from another dataset:

```yaml
# Generated from: grid.yml
# Do not edit this file by hand. Edit the source file instead!

status:
  baseline: high
  baseline_low_date: 2017-10-17
```

"You never edit a dist file directly." The check is the generator itself with a flag: `"test:dist": "tsx scripts/dist.ts --check"`. The schema is one JSON Schema file, and the TypeScript types are generated from it.

**What it gets right for Silo.** It answers the question the Workshop has with verdicts and issue numbers: a value that comes from somewhere else is kept in its own file, written only by a tool, next to the authored file, and a check regenerates and compares. Two kinds of writer never touch the same file. It also states when a generated file should be committed: "Even if dist files are generated artifacts, they are still checked-in … we review them as part of the pull request process."

**What would not fit.** YAML needs a parser dependency here. It has no free text per feature and no order.

**To borrow.** Authored file and tool-written file side by side; the `--check` mode; the header; one schema that generates the types.

### 2.5 GitHub issues: truth or mirror

Every tool that touches an external tracker had to pick, and the ones that did not pick say so in their bug trackers. Beads syncs both ways and loses fields on the round trip (#1573). Backlog.md refused. Spec Kit and CCPM write one way and never read back. Kubernetes shares one number between the KEP folder and its issue and keeps status in both.

For Silo the question is per field, not per system:

| Fact | Who can change it | Natural owner | The other side |
| --- | --- | --- | --- |
| Name, sentence, area, stage, needs, order, properties, behaviour text | Owner with an agent, through the gate | Repository | Issue title, body, type, "blocked by": written by `sync`, restored when edited on GitHub (already the case for title and body) |
| Votes, comments, suggestions | The public | GitHub | Read live by the page; never stored |
| Verdict | Owner, on the page or in conversation | One of the two, not both (today: both) | If GitHub: the repository keeps a tool-written copy with who and when, in its own file, so an agent offline can obey "nothing is built without an accepted entry", and a check compares. If the repository: every click is a commit on `main`, which `docs/WORKSHOP.md` already rejected |
| Issue number | GitHub | GitHub | Derivable: `sync` already finds an issue by its marker, and `GET state` returns the entry id of every issue. Stored in the repository it is a cache |
| Being built | The orchestrator, across worktrees | A live place (a label today). A file in a branch cannot say it: other worktrees do not see it until it merges | |
| Built | The merge | Repository, changed in the same merge as the code | Closing the issue is then generated by `sync` |

## 3. What recurs, and what goes wrong

### Patterns in the ones that work

1. **One file per item; the id is the file name.** Backlog.md, ticket, beans, web-features, KEPs, PEPs, changesets, towncrier, Go's `doc/next`, OpenSpec changes. The reason given is always the same one: "having one single file which developers all write to and produce merge conflicts" ([towncrier](https://github.com/twisted/towncrier)); "Multiple changes can exist simultaneously without conflicting" ([OpenSpec concepts](https://github.com/Fission-AI/OpenSpec/blob/main/docs/concepts.md)).
2. **Ids are names or hashes, never positions.** Slugs (web-features, KEP folder names), hashes (Beads). Every tool with sequence numbers has an issue about collisions or renumbering (Backlog.md #711, Spec Kit #1066 and #4065).
3. **What can be computed is not stored.** Readiness and the dependency graph (Backlog.md), `bd ready`, PEP 0. Position in a list belongs here too.
4. **Files are the truth, or the database is; never both.** The tools that came after Beads say it in their first paragraph: "No daemon, no binary DB files. The JSONL file IS the database" (Seeds); "without the need for keeping a SQLite file in sync or a rogue background daemon mangling your changes" (ticket).
5. **Generated output is either not committed (PEP 0, Astro's `.astro/`, Backlog.md's readiness) or committed with a header and checked by running the generator with `--check`** (web-features; Go's cross-check of `api/next` against `doc/next`).
6. **Files are split by who writes them.** Authored `.yml` and generated `.yml.dist`; KEP prose in `README.md` and data in `kep.yaml`; OpenSpec's `.openspec.yaml` beside its Markdown.
7. **One entry command, wired from the agent file, and JSON everywhere.** `bd prime`, `sd prime`, `beans prime`, `backlog instructions overview`, `openspec instructions --json`. Backlog.md makes the CLI the default and MCP optional.
8. **Tool-owned files are protected from hand edits.** Seeds installs a hook that "denies hand edits to `.seeds/*.jsonl`"; web-features uses a header; Backlog.md asks for commands "over hand-editing task files, so field types and metadata stay consistent".
9. **A mirror goes one way, and one marker ties the two sides.** KEP number = issue number; CCPM renames files to issue numbers; Silo's `<!-- silo-entry: id -->`.
10. **Errors are data.** OpenSpec's `{severity, code, message, target, fix}`; Rust's `{file}:{line}: no tracking issue for feature {name}`.
11. **A requirement has an id, tests point at it, and a tool lists what is unproven.** OpenFastTrace tags, Doorstop links with fingerprints, Rust tidy ("All unstable lang features have tests to ensure they are actually unstable"), Kiro's properties.
12. **Schema changes are cheap when optional keys are absent rather than null, unknown keys have a stated fate, and the data says which schema version it is.** OpenSpec contract; Backlog.md #918; Beads' envelope and version guard.

### Failure modes people report

| What went wrong | Where | Source |
| --- | --- | --- |
| A shared list file conflicts on every parallel change | Beads' JSONL era; the reason towncrier and changesets exist; `site/workshop-map` here | [beads #158](https://github.com/gastownhall/beads/issues/158), section 0 |
| Two stores kept in step lose data | Beads export and import; three checkouts, three counts | [#1623](https://github.com/gastownhall/beads/issues/1623), [#3135](https://github.com/gastownhall/beads/issues/3135) |
| Truth outside the git diff is invisible to review and fragile | `refs/dolt/data` destroyed by `git push --mirror`; stale reads | [#5266](https://github.com/gastownhall/beads/issues/5266), [#4746](https://github.com/gastownhall/beads/issues/4746) |
| Sequence numbers collide or force renumbering | Backlog.md, Spec Kit | [#711](https://github.com/MrLesk/Backlog.md/issues/711), [#997](https://github.com/MrLesk/Backlog.md/issues/997), [#1066](https://github.com/github/spec-kit/issues/1066), [#4065](https://github.com/github/spec-kit/issues/4065) |
| A generated merge drops content without a warning | OpenSpec archive | [#1246](https://github.com/Fission-AI/OpenSpec/issues/1246) |
| A spec written per feature goes stale when later features overlap it | Spec Kit | [#620](https://github.com/github/spec-kit/issues/620) |
| A tool rewrites a file from its own model and drops keys it does not know | Backlog.md | [#918](https://github.com/MrLesk/Backlog.md/issues/918), [#1172](https://github.com/MrLesk/Backlog.md/issues/1172) |
| Generated files that someone must remember to regenerate | Task Master | [task-structure.md](https://github.com/eyaltoledano/claude-task-master/blob/main/docs/task-structure.md) |
| A tool writes to the main checkout from a worktree | Backlog.md MCP | [#558](https://github.com/MrLesk/Backlog.md/issues/558), [#689](https://github.com/MrLesk/Backlog.md/issues/689) |
| Two-way sync with a tracker is lossy | Beads | [#1573](https://github.com/gastownhall/beads/issues/1573) |
| A schema migration run on two clones forks the data | Beads | [#4259](https://github.com/gastownhall/beads/issues/4259) |
| Agents do not mark work done, which blocks what depends on it | Claude Code agent teams: "teammates sometimes fail to mark tasks as completed, which blocks dependent tasks" | [docs](https://code.claude.com/docs/en/agent-teams) |
| Claims in one shared file with locks: locks held too long or never released, and agents edited the file without the lock | Cursor, 2026-01-14: "Our initial approach gave agents equal status and let them self-coordinate through a shared file." They moved to planners that create tasks and workers that only do one | [Scaling long-running autonomous coding](https://cursor.com/blog/scaling-agents) |
| Claims as files synced through git work, at the price of conflicts | Anthropic, 2026-02-05, 16 agents: "Claude takes a "lock" on a task by writing a text file to current_tasks/"; "Merge conflicts are frequent" | [Building a C compiler](https://www.anthropic.com/engineering/building-c-compiler) |
| A hand-kept plan file drifts and is thrown away | Ralph, 2025-07-14: "I have deleted the TODO list multiple times"; the prompt itself says the plan "may be incorrect" | [ghuntley.com/ralph](https://ghuntley.com/ralph/) |
| One long line of JSON per item is a poor thing to hand an agent | ticket: "without dumping ten thousand character JSONL lines into their context window" | [ticket README](https://github.com/wedow/ticket) |
| `merge=union` as a shortcut for shared lists | Git's own text: "This tends to leave the added lines in the resulting file in random order and the user should verify the result. Do not use this if you do not understand the implications." | [gitattributes](https://git-scm.com/docs/gitattributes) |

## 4. Adopt, borrow, build

### Adopt as is

| What | Why | Confidence |
| --- | --- | --- |
| Git and plain files as the only store of the plan; no database, no cache | Every failure in 2.3 comes from a second store. At 120 features a full read is milliseconds. If a query is ever wanted, an in-memory `node:sqlite` built from the files on each run needs no dependency and can never be stale | High |
| JSON for the structured part | The only format read by the standard library of Node 26, Python 3.11+, Zig and Swift (section 0). Pretty-printed, one key per line, so git's unit of conflict becomes the field | High |
| JSON Schema as the one description of the fields, checked with ajv pinned to an exact version | The schema is data, so the field documentation, the editor completion and any Python or Zig reader come from one file instead of five lists. ajv is MIT, widely used, development-time only and reads public files. The alternative is to keep one hand-written validator shared by site and tool: no dependency, but the schema stays code | Medium |
| GitHub issues for votes, comments and the owner's click, as decided on 2026-11-16; optionally issue type Feature and native "blocked by" in the mirror | Already built. The native dependency endpoints exist in REST, so "needs" can show on GitHub without a second truth, as long as only `sync` writes them | High for keeping it; medium for the two additions (more calls, more to restore) |
| changesets for release notes | Already in use; nothing to change | High |
| None of the trackers or spec kits as the plan's store | Beads and Task Master fail on store or license; Backlog.md and OpenSpec model a task and a behaviour spec, not a feature with a verdict, and each would need a second tool beside it | High for Beads and Task Master; medium-high for Backlog.md; OpenSpec is worth another look when features get behaviour specs |

### Borrow

| What | From | Confidence |
| --- | --- | --- |
| One folder or file per feature, id = name, never renamed; a dropped feature keeps its id | web-features, KEPs, Backlog.md's archive | High |
| Separate files by writer: authored definition, authored prose, tool-written values (verdict copy, build state), each with one kind of writer | web-features `.yml` and `.yml.dist` | High |
| Order kept in one place and position derived. Either one list of ids per stage (moving a feature moves one line; only the planning conversation edits it) or a sparse number per feature as Backlog.md does | Backlog.md `ordinal`; Go's "sorted by pathname" | Medium on which of the two |
| `needs` as a list of ids on the feature; `ready`, `blocked` and cycles computed and failing closed | Backlog.md readiness, `bd ready` | High |
| One decision per file with date, who, the features it touches and what it supersedes; the list generated | MADR front matter, changesets | High |
| A `prime`-style command named in AGENTS.md that prints the fields, the commands and the current state, generated from the schema; `show <id> --json` that returns one feature with its prose, verdict, decisions and what needs it; one JSON document per call inside `{schema_version, data}` | Beads, Backlog.md, OpenSpec | High |
| Errors as `{code, message, target (file and field), fix}` | OpenSpec agent contract; the site's parser already prints file and line | High |
| Fixed headings in the prose, checked; a greppable marker for an open question | OpenSpec `validate`, Backlog.md section markers, Spec Kit `[NEEDS CLARIFICATION: …]` | Medium |
| Anything generated and committed gets a header and a `--check` run at the gate; a hook refuses hand edits to tool-written files | web-features, Seeds `sd guard` | High |
| Later: a property is a named requirement with scenarios; a test names the property it proves in a comment or its name; a link remembers the property's fingerprint so a changed property flags its tests | OpenSpec grammar, OpenFastTrace tags, Doorstop fingerprints, Rust tidy | Medium |
| "Being built" stays a live mark set by the orchestrator; "built" lands in the repository with the merge and is mirrored out | Section 2.5; Anthropic and Cursor reports | Medium |
| Optional keys absent, not null; the schema version written once; a schema change is one commit that changes the schema, migrates the files and passes the check | OpenSpec contract, Beads version guard | Medium-high |

### No existing answer: to build

| What | Why nothing covers it | Confidence that it must be built |
| --- | --- | --- |
| The registry command itself (read, list, ready, edit one field, check, prime) | No tool found holds a product's features with a verdict, an area and a build order; all hold tasks or specs | High (about 25 tools looked at) |
| The checks across files: unknown ids, cycles, every feature in the order exactly once, recordings present, schema version, prose that quotes a count | Schema languages stop at one file; CUE could do part of it at the price of a new language | High |
| One owner per field between repository and GitHub, with the verdict copy written only by the tool | Every tool either refuses the sync or loses data in it; `tools/workshop` already does the hard half | High |
| The property-to-test tracer for Zig and Swift | OpenFastTrace does the job but needs a JVM; whether its tag importer reads `.zig` and `.swift` was not checked. A scanner for tags is small | Medium |
| Counts and state inside prose pages (`PRODUCT.md`, `RESTART.md`) | No tool; either generate those lines or forbid numbers in prose and check | Medium |

## 5. Not verified

- OpenAI, "Harness engineering" (2026-02-11): the page answered 403 and a reader proxy was refused, so nothing in this report rests on it. From memory and second-hand summaries only: AGENTS.md kept to about a hundred lines as a table of contents, `docs/` as the system of record, plans versioned in the repository, linters and a recurring agent that look for stale docs.
- Steve Yegge's posts on Medium (403). The Beads history above comes from its issue tracker and docs instead.
- MDN browser-compat-data's guidelines page (404); it is listed nowhere above as evidence. The content-collections docs site (429); only its README was read.
- Kiro: where spec files are stored and the EARS format were not on the pages opened; that it is proprietary is inferred from the missing license.
- GitHub: whether a GitHub App's installation token may write issue field values (the docs say "users with push access"); the number of issue fields per organization (a search result said 25, the GA post gives no number); limits and cross-repository rules for issue dependencies; the 65,536-character limit of an issue body (from memory); whether GitHub's own merges honour `merge=` attributes.
- Task Master, Seeds, ticket, beans and OpenSpec have no GitHub sync as far as their READMEs, docs listings and CLI references show; their code was not searched.
- Whether the fixes for Backlog.md #918 and OpenSpec #1246 work: both are closed, neither was run.
- BMAD, Agent OS, cc-sdd, sudocode, chainlink, vibe-kanban, Sphinx-needs, StrictDoc and CUE were judged from metadata and READMEs, not used.
- Zig `std.json` and Swift `JSONDecoder` reading the same files: from knowledge, not run. No tool in this report was installed or run; every fit verdict is from reading.
- Zed and "Linear as code": no primary source found; not examined. Docusaurus plugins: not examined.
- Star counts, release dates and open-item counts are the GitHub API's answers on 2026-11-16.
