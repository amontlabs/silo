# Reviewing a fork's changes

A fork is compared with its parent as it was at fork time. The guest writes a state
manifest at both points ([format 2](v1/PHASE2_CONTRACT.md)); the engine compares them with
`changes::diff` (`app/SiloUI/src-tauri/engine/src/changes/`) and shows the result as buckets.
`silo diff` prints the text, `silo diff --json` the full detail.

## Buckets

| Bucket | What it holds |
| --- | --- |
| Code | One line per git repo: new or removed repo, moved head and branch, dirty and untracked files, changed gitignored files (`~/app: .env changed (gitignored)`). Files under a repo are reported only here. |
| Packages | One line per manager (`apt`, `brew`, `brew cask`, `npm -g`, `pip`, ...). Added packages are named; their transitive dependencies fold into the manual install (`apt: ripgrep, jq (+3 dependencies)`). Upgrades and removals follow. Files that a package installed (`OWN` records) count toward the package, never toward Other changed. |
| Settings & files | Dotfiles, `/etc`, preferences, crontab (one `cron <user>: added` line, not the crontab file as well), launch agents, service state changes. Directories with more than three changed files roll up to one line. |
| Database changed | Postgres: `schema changed` and `data changed in <table> (10→12 rows)`; SQLite: the file digest before and after. |
| Other changed | Everything else, rolled up by directory. |
| Not kept | What a keep never brings back: new listening ports, processes, services, and credential paths (`~/.ssh/`, `~/.aws/credentials`, `~/.config/gh/hosts.yml`, `~/.openclaw/credentials/`, `*.pem`, keychains; the list is in `changes/ignore.rs`). |
| Ignored | A count of paths that are never reviewed (logs, caches, build output, boot-time rewrites; also in `ignore.rs`). |

A no-op baseline (the diff of a fork of the same source left idle for the same time) can be
passed in; entries that match it are removed first, so background churn does not show.

The text is at most 15 lines for a typical task. Longer sections are cut longest-first and end
in `… +N more (see --json)`.

## `silo diff --json`

`Diff` serializes to one object. Fields never change meaning within `format: 1`; new fields
may be added.

```json
{
  "format": 1,
  "home": "/root",
  "summary": [{ "bucket": "Code", "lines": ["~/app: 46cc034..e710c1a"] }],
  "code": [{
    "repo": "/root/app",
    "new": null, "removed": false,
    "commits": ["46cc034...", "e710c1a..."], "branch": null,
    "dirty_new": { "src/routes.js": "_M" }, "dirty_gone": [], "dirty_delta": 0,
    "gitignored": [{ "path": "/root/app/.env", "change": "modified" }]
  }],
  "packages": [{
    "manager": "dpkg",
    "added": { "jq": "1.7.1" }, "removed": {}, "upgraded": { "curl": ["8.1", "8.5"] },
    "manual": ["jq"], "requires": {}, "owned_files": { "jq": ["/usr/bin/jq"] },
    "top_level": ["jq"], "dependencies": ["libjq1"]
  }],
  "settings": {
    "files": [{ "path": "/etc/app.conf", "change": "added" }],
    "cron": [{ "user": "root", "change": "added" }],
    "services": [{ "name": "nginx.service", "from": "enabled", "to": "disabled" }],
    "launch_agents": [{ "scope": "user", "label": "dev.x", "change": "added" }]
  },
  "database": [{
    "engine": "postgres", "name": "app", "change": "modified",
    "schema": ["<before sha256>", "<after sha256>"],
    "tables": [{ "table": "users", "change": "modified", "rows_before": 10, "rows_after": 12 }],
    "digest": null
  }],
  "other": [{ "path": "/usr/local/bin/mine", "change": "added" }],
  "not_kept": {
    "listening": [{ "proto": "tcp", "port": "3001", "process": "node" }],
    "processes": ["node"], "services": [],
    "credentials": { "/root/.ssh/": ["/root/.ssh/id_ed25519"] }
  },
  "repo_files": ["/root/app/src/routes.js"],
  "ignored": { "paths": ["/var/log/syslog"], "pruned_trees": [{ "path": "/root/.cache", "delta": 5 }] }
}
```

- `change` is `added`, `modified` or `deleted`.
- `summary` holds the text lines of each non-empty bucket before any line limit, in display order
  (`Code`, `Packages`, `Settings & files`, `Database changed`, `Other changed`, `Not kept`).
  The `Ignored` line is `ignored.paths.length` plus the `delta` of every pruned tree.
- `other` and `settings.files` list every changed path; the text rolls them up.
- `repo_files` lists changed files inside repos; they are not shown outside Code.
- `database[].schema` and `digest` are `[before, after]` with `null` for a missing side.

## Manifest additions the diff reads

On top of [format 1](v1/PHASE2_CONTRACT.md) the diff uses `GIG`, `DB`/`DBT`, `CRONF`, `LAGENT`
and `OWN` records, plus an optional dependency record, `PKGD mgr name dep,dep,...` (direct
requirements, one line per package). Without `PKGD`, apt and brew fold every added package that
is not in `PKGM` into one `(+N dependencies)` count; with it, each top-level package carries its
own count. pip has no manual marker: with `PKGD`, the packages nothing else requires are the
top level; without it, the line shows the first three names and `(+N more)`.

A manifest with a `META format` above 2, or a known record with too few fields, is rejected
instead of read as a smaller state.

## Validation

The recorded validation manifests (Linux `l1`..`l5`, `s1`, `s2`; macOS steps; one live agent
task) are kept in `changes/fixtures/`, reduced to the records that differ between the two sides,
and `changes/tests.rs` asserts the exact text for each. Line counts against the first
prototype: 3, 4, 7, 15, 9, 8 and 11 on Linux (the PDF skill moved from 10 to 11 because its
credentials file now has its own Not kept line), 14, 12 and 12 on macOS, 3 for the live task.

## Keeping a fork: `silo fold` and `silo undo`

`silo fold <fork> [--into main] [--dry-run] [--json]` keeps a fork's changes into the computer it
was made from (`changes.keep`, `app/SiloUI/src-tauri/engine/src/changes/keep/`). `silo undo <computer>`
puts the computer back to before its latest keep (`changes.undo`). Both are MCP tools (`fold`, `undo`).

A keep reads three manifests (the fork-time base, the fork now, the target now) and builds its
plan from them; the diff supplies the item list (never Not kept or Ignored) and the database check.

1. **Checks, before anything changes.** For each file, package, crontab line and launch agent the
   keep would touch: if the target changed it since the fork point and the fork ended differently,
   it is a conflict. Git: the fork's commits are bundled into the target and the merge is tried
   (`git merge-tree`); a conflicting merge is a conflict. A database change (schema or data) with
   no changed migration file and no `keep: migrate:` step is refused, pointing to "Make this main".
   A refusal is a result (`status: "refused"`, exit code 2), the target is untouched.
2. **Checkpoint.** A `before-keep` checkpoint of the target (macOS computers use a manual
   checkpoint named `Before keep of <fork>`), and a journal entry naming it.
3. **Apply, in order.** Packages by name and version (`apt-get install name=ver`, `brew install`,
   `npm -g`, `pip`, `pipx`, `cargo`; package files are never copied), git merge and uncommitted
   files, files (written next to the target and renamed; links recreated; files identical after the
   package replay skipped), crontabs merged by line against the fork-time text (an edited line
   appears once, a deleted line is gone, lines only the target has stay), launch agents, then the
   recipe's `migrate:` commands (always, when set), `restart:` services (the process id must change
   and the new process must run), and `after:` commands.
4. **Failure.** A failing step restores the checkpoint and reports the error. If the engine stops
   mid-keep, the journal lets the next keep, `silo undo`, or engine start restore the checkpoint.
5. **One keep at a time per target.** A second waits up to two minutes, then fails with `busy`.

`silo undo` restores the latest before-keep checkpoint a keep made (the engine's restore saves a
"Before restore" checkpoint first) and does not use the same one twice.
