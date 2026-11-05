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
| Settings & files | Dotfiles, `/etc`, preferences, crontab (one `cron <user>: added` or `cron <user>: 1 line added` line, not the crontab file as well), launch agents, service state changes. Directories with more than three changed files roll up to one line. |
| Database changed | Postgres: `schema changed` and `data changed in <table> (10→12 rows)`; SQLite: the file digest before and after. |
| Database not compared | A Postgres database whose side of the comparison could not be read (the server did not answer when the fork-time base was captured): `postgres appdb: database not compared: base unreadable (...)`. It is not listed as new, dropped or changed. |
| Other changed | Everything else, rolled up by directory. |
| Not kept | What a keep never brings back: new listening ports, processes, services, and credential paths (`~/.ssh/`, `~/.aws/credentials`, `~/.config/gh/hosts.yml`, `~/.openclaw/credentials/`, `*.pem`, keychains; the list is in `changes/ignore.rs`). |
| Ignored | A count of paths that are never reviewed (logs, caches, build output, boot-time rewrites; also in `ignore.rs`). |

A no-op baseline (the diff of a fork of the same source left idle for the same time) can be
passed in. `silo diff --baseline` measures a missing one on a throwaway fork of the same
checkpoint (named `silo-baseline-*`, quiet like any fork, left out of `silo list`): it is started so
its base is captured like a real fork's, left idle for as long as the real fork had run (at most
ten minutes; `baselineIdleSeconds` records it), captured again, and the idle diff is cached under
the source checkpoint. The throwaway is dropped whether or not the measurement worked. A cached
baseline is used as it is. On a Mac with two macOS virtual machines already running nothing is
measured. `changes.baseline` itself refuses ("No baseline") on a fork that has run more than two
minutes past its idle time or whose capture shows a moved repository or changed package, because
that capture would hold the task's own changes. When no baseline can be measured, `silo diff`
shows the plain diff and a `baselineError`. Entries that match the baseline are removed first, so background churn does not show; entries that match it are removed first, so background churn does not show. An
entry matches only when the path, the kind of change and the resulting content (file hash,
size and link target; Postgres schema and table data digests; SQLite file digest; crontab and
launch agent digests; the working-tree content of a dirty file) are all the same. A path that
ends up with different content in the task than in the baseline stays in the diff.

SQLite files go through the same ignore and credential rules as other files: a database under an
ignored tree is only counted, and one under a credential path appears under Not kept only.
Paths from `git status` are unquoted (git's C-style quoting) and the `old -> new` form is read
only for rename and copy status codes. Editing a file that was already dirty shows it under Code
again when its content changed.

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
- Every path entry (`settings.files[]`, `other[]`, `repo_changes[]`, `code[].gitignored[]`,
  `packages[].owned_changes{pkg: []}`, `not_kept.credential_details{group: []}`) carries
  `before` and `after`, each `null` or `{kind: "file"|"link", size, hash?, target?, mtime?}`
  (`hash` is absent for files too large to hash, which then carry `mtime`; `target` is a
  symlink's destination). For packages, the entry's `change` is the operation on the package's
  file; `owned_files` keeps the plain path lists.
- `code[]` also has `removed_from` (head and branch of a removed repo), `dirty_identity`
  (repo-relative path to the content now) and `renamed_from` (new path to old path).
- `settings.cron[]` and `settings.launch_agents[]` carry the exact `path` of the crontab file or
  plist and the `before` and `after` digests. `database[].tables[]` carries `digest_before` and
  `digest_after`; row counts are informational only.
- `not_kept.credentials` is derived from `credential_details` (paths only, as before).
- `summary` holds the text lines of each non-empty bucket before any line limit, in display order
  (`Code`, `Packages`, `Settings & files`, `Database changed`, `Database not compared`,
  `Other changed`, `Not kept`).
  The `Ignored` line is `ignored.paths.length` plus the `delta` of every pruned tree.
- `other` and `settings.files` list every changed path; the text rolls them up.
- `repo_files` lists changed files inside repos; they are not shown outside Code.
- `database[].schema` and `digest` are `[before, after]` with `null` for a missing side.
  `database[].not_compared` is set (with the reason) when one side's server could not be read.
- `settings.cron[]` may carry `lines_added` and `lines_removed` when both crontab texts are
  known. A crontab the quiet step parked is compared with its parked original.

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

`silo fold <fork> [--into main] [--dry-run] [--reviewed DIGEST] [--data-ok] [--json]` keeps a
fork's changes into the computer it was made from (`changes.keep`,
`app/SiloUI/src-tauri/engine/src/changes/keep/`). `silo undo <computer>` puts the computer back
to before its latest successful keep (`changes.undo`). Both are MCP tools (`fold`, `undo`).

A keep builds its plan from three manifests (the fork-time base, the fork, the target); the
diff supplies the item list (never Not kept or Ignored) and the database check. Files are
compared live: both computers are hashed at keep time, so a file the collector could not hash,
an unreadable file or a changed mode is never taken as unchanged.

1. **Pinned to what was reviewed.** `silo diff --json` returns a `reviewDigest`; `--reviewed`
   passes it back, and the keep refuses (`changed`) if the fork's changes differ. Whatever the
   keep writes is read from the fork once, before main is touched, and the fork is looked at
   again afterwards: if it moved meanwhile, the keep stops.
2. **Checkpoint, then lane.** A `before-keep` checkpoint of the target (macOS computers use a
   manual checkpoint named `Before keep of <fork>`), then the target's operation lane is held
   until the apply ends, so no restore, stop, fork or delete of it interleaves. The target is
   captured after the lane is taken. Every check below that does not change the target runs once
   before the checkpoint too, so a refused keep is quick and creates nothing; the checks run
   again once the lane is held. On Linux a keep reads and writes as root, and runs repository
   commands and recipe steps as the owner of the folder they work in.
3. **Checks, before anything is written.** Per file, package, crontab line and uncommitted file:
   if the target changed it since the fork point and the fork ended differently, it is a
   conflict (uncommitted files are compared with the blob at the fork point, so a commit on
   main counts). Git history is searched for credentials, `.env` files and ignored paths; any
   found refuses the keep (`credentials`) with the list. The fork's commits are fetched into a
   temporary ref and tested with `git merge-tree`; a conflict, or a git that cannot test,
   refuses. Destinations are checked: a link or non-folder in the path, or a folder where a
   file goes, refuses. Package names are validated; brew installs only when it would install
   the fork's version. A database change is carried only with a `keep: migrate:` step and
   changed migration files (folders named `migrations`, `migrate` or `alembic`), or `--data-ok`;
   otherwise the refusal points to "Make this main". A refusal is a result
   (`status: "refused"`, exit code 2) and the target is untouched.
4. **Apply, in order.** Package installs and removals (each manager as the user it belongs to),
   git merge, uncommitted files and deletes (renames delete the old path), files (written next
   to the target and renamed; links recreated, never read through), crontabs merged by line
   against the fork-time text (`CRONTXT` in the base manifest), launch agents (a plist that
   will not load fails the keep), then the recipe's `migrate:`, `restart:` (the process id must
   change and the new process must run) and `after:`.
5. **Failure and crashes.** A failing step restores the checkpoint, which then can no longer be
   undone. The journal (synced to disk) lets the next keep, `silo undo`, or the engine's start
   restore the checkpoint of an interrupted keep.
6. **One keep at a time per target.** A second waits up to two minutes, then fails with `busy`.

`silo undo` restores the latest checkpoint of a keep that succeeded, once, and lists what
changed on the computer after the keep (that is discarded). Temporary bundles and git refs are
removed on every path.
