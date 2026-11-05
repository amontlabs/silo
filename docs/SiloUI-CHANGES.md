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
the source checkpoint and under the computer the fork was made from, so every fork of that
computer finds it (a macOS fork has a fork-point checkpoint of its own). This is how a baseline is
measured for a fork that has been used: the measurement never runs on the used fork. The
throwaway is stopped, seen stopped, and dropped, whether or not the measurement worked; a
failure to drop it is reported only if it remains after retries. A cached baseline is used as it
is. On a Mac with two macOS virtual machines already running nothing is measured. The engine
method `changes.baseline` itself, called on a used fork, still refuses ("No baseline") when the
fork has run more than two minutes past its idle time or its capture shows a moved repository or
changed package, because that capture would hold the task's own changes; `silo diff --baseline`
does not call it on the used fork. When no baseline can be measured, `silo diff` shows the plain
diff and a `baselineError`.

Applying a baseline, in order:

1. Entries that match it are removed. An entry matches only when the path, the kind of change and
   the resulting content (file hash, size and link target; Postgres schema and table data
   digests; SQLite file digest; crontab and launch agent digests; the working-tree content of a
   dirty file) are all the same.
2. Any file outside Code, Packages and Database (Settings & files and Other changed) whose path
   also changed in the idle baseline, with any content, moves to `idle_noise`: it is counted in
   the text under "Also changes when idle" and listed by path (with its identities) in the JSON,
   not in a bucket. Code, Packages and Database entries that end up different from the baseline
   stay in the diff.

Built-in ignore patterns (`changes/ignore.rs`) cover well-known macOS background churn seen on an
idle macOS fork: `~/Library/Preferences/ByHost`, `com.apple.*` preference files, contacts and
knowledge databases and their `-shm`/`-wal` files, system management state, Biome, Daemon
Containers, Spotlight metadata, logs. macOS permission grants (`com.apple.TCC`) are reported under
Not kept, never as a database change.

SQLite files go through the same ignore and credential rules as other files: a database under an
ignored tree is only counted, and one under a credential path appears under Not kept only.
Paths from `git status` are unquoted (git's C-style quoting) and the `old -> new` form is read
only for rename and copy status codes. Editing a file that was already dirty shows it under Code
again when its content changed.

The text is at most 15 lines for a typical task. Longer sections are cut longest-first and end
in `… +N more (see --json)`. Within a section, lines for single files, launch agents, crontabs
and services come before the directory counts (`~/Library/Preferences/ (17 files: ~17)`), so a
cut drops the counts first.

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
  `before` and `after`, each `null` or `{kind: "file"|"link", size, hash?, target?}`
  (`hash` is absent for files too large to hash; those are compared by size and modification time, which are not part of the entry; `target` is a
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
   passes it back, and the keep refuses (`changed`) if the fork's changes differ. The digest is a
   SHA-256 over the reviewed content only: the Code, Packages, Settings & files, Database and
   Other changed entries as shown after the ignore list and the baseline, with their identities
   (hash, size, link target). It leaves out the Ignored count, idle noise, Not kept, and every
   timestamp (an entry carries no modification time). `silo fold --dry-run` does not send a digest, so
   it never refuses on a stale one: a dry run reports and does not pin. Whatever the
   keep writes is read from the fork once, before main is touched, and the fork is looked at
   again afterwards: if it moved meanwhile, the keep stops.
2. **Checkpoint, then lane.** A `before-keep` checkpoint named `Before keep of <fork>` of the
   target (on both platforms, with the same retention count; a running computer is captured
   with its memory, a Linux one with `--full --guest-flush required`), then the target's operation lane is held
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
changed on the computer after the keep (that is discarded). A restore leaves the computer
stopped, so the undo then starts it when it was running at the keep (the checkpoint has memory):
the start resumes the checkpoint's memory, processes and open databases as they were. A
computer that was stopped stays stopped. The result says which (`main`, `resumedFrom`) and
carries a note when main was put back but could not be started. A restore has no disk-only
fallback for a checkpoint with memory. Temporary bundles and git refs are
removed on every path.

## Making a fork main: `silo fold <fork> --promote`

`silo fold <fork> --promote [--into main] [--dry-run] [--json]` (MCP `fold` with `promote`) makes
the fork the computer a keep would have changed (`changes.promote`,
`app/SiloUI/src-tauri/engine/src/changes/promote/`). The fork takes main's name and settings;
main is archived as `main-before-<yyyymmdd-hhmm>` (a number is added if taken), stopped, with
its checkpoints. `silo undo main` (or `silo undo <fork>`) swaps everything back.

A promote refuses, changing nothing, when either computer is busy, the two run different
systems, they carry the same name, or (Linux) one of them keeps a disk in a folder named after it. Otherwise it saves a
`Before promote of <fork>` checkpoint of main, stops both, holds both operation lanes (in id
order), writes the journal (`changes/promote/<fork id>.<main id>.promote.json`, synced) and
applies these steps, recording each:

| Step | What moves |
| --- | --- |
| names | the fork becomes `main`, old main becomes the archive name (computer record on macOS). A rename that would give two computers one name fails, so a duplicate `main` cannot exist even between the two renames. Linux: `msb rename` of the stopped main (to the archive name) and then of the stopped fork (to `main`), then both saved names in one write. Each sandbox is identified by the `silo.machine-id` label it carries, never by its name, so a repeat after a crash renames only what is not renamed yet; the SSH entries are rewritten here too (below) |
| name-keyed settings | secret assignments (`secrets.rs`) and the GitHub assignment (`github.rs`) are keyed by name, so main's stay with the name `main`, now the fork's; the fork's own grants under its old name are dropped like a removed computer's (saved choices, cached attachments and issued tokens), never added to main's. Published ports move from main's id to the fork's on macOS (`macos-computers/ports`, keyed by id); on Linux they are saved under the computer name (`network.json`), so main's stay with the name `main`, now the fork's, and the fork's own are removed (an undo gives them back); URL routes (`url_proxy`, hosts derived from the name) are forgotten and rebuilt from the new name |
| SSH access | the saved SSH entries are named after the computer id (`silo-<id>`), so main's entry is rewritten to connect to the new main (with its key and pinned host key) and the archive's to the archive; only entries that exist are touched. Linux entries connect by computer name and pin that sandbox's host key, so they are rewritten at the end of the names step, after both sandboxes carry their final names (an undo rewrites them back the same way) |
| lineage | read from the journal, nothing is rewritten: while a promote is in force, forks made from the old main resolve to the new main (so `silo fold` and keep of those forks target it), and the new main's own source is the old main's. An undo ends the record and every answer returns |
| macOS network identity | the new main presents main's proxy CA (the trust set up for main covers it; the fork's own CA is set aside in the Keychain and comes back on undo) and main's hostname, written to the stopped disk's system configuration; the archive takes the fork's hostname. The machine identifier stays with each disk: Apple ties a guest's hardware identity and what it activated to the identifier it first booted with, and the disks swap names, not hardware |
| quiet parking | macOS: the scheduled work parked when the fork was made quiet is put back from `Library/Application Support/Silo/Quieted/` on the stopped disk, except at paths the fork has used since (those stay parked and listed). Linux: the new main is started, the cron, at, anacron and systemd timer items under `/var/lib/silo-quiet/disabled` are moved back through the guest (`quiet_restore.sh`, as root) except at paths the fork has used since, and it is stopped again; a timer the quiet step only masked is unmasked, and `/etc/crontab` gets its parked text back when it still holds nothing but comments. Items parked for the services the fork was asked to stop stay parked. Best effort; it never fails a promote |

Keyed by id and therefore untouched: changes store manifests, checkpoint records and the
proxy address slot.

`silo undo` takes back the newest of a promote and a keep of that computer (an interrupted
keep first). Undoing a promote:

- reverts only the latest promote that involves either computer; an older one is refused with
  "Undo that one first" while a later promote built on it;
- refuses, changing nothing, when the two no longer carry the names the promote gave them, or
  when another computer took the fork's old name, so two computers are never named `main`;
- puts back what the promote moved and keeps what happened to the main role since: ports
  changed on the new main while it was main go back to the old main with the name, and the
  fork gets the ports it had. Secrets and GitHub settings are keyed by name and follow the
  name. Not restored exactly: a secret value or GitHub token issued while promoted lives in the
  Keychain under the name and stays with it; the fork gets its recorded assignments back, and
  its GitHub runtime identity is created at its next start.

A crash is repaired when the engine starts: a promote with every step but quiet parking
recorded is rolled forward, any other is rolled back (steps are idempotent and reverted in
reverse order); an interrupted undo is finished. Both computers are held (their operation
lanes) from engine start until their recovery is done, and the recovery retries while the
runtime is not ready, so a half-swapped computer cannot start. The next promote or undo of
either computer settles it first.

Linux computers are promoted with the same steps. The runtime names each sandbox after its
computer, so the names step uses the stopped-sandbox rename of the bundled `msb`
(`microsandbox-rename-stopped`, see [runtime packaging](SiloUI-RUNTIME-PACKAGING.md#rename-a-stopped-sandbox-2026-11-05)).
Checkpoint records, SSH aliases' file names and changes manifests are keyed by computer id and
need no change; the snapshot group is stored on the computer's record and keeps its name.

Known gaps:

- A Linux computer whose configuration still mounts a disk from `volumes/<name>/` (not yet
  converted to a disk the computer owns) is refused with `storage`: the folder is named after the
  computer and a delete by name would remove the wrong one.
- Linux quiet parking is restored only for scheduled jobs. A masked timer is unmasked only when
  its mask link was made between the quiet step's first and last file (`<token>.machine-id` and
  `applied-<token>`), or is listed in `masked-timers.list` if the quiet step ever writes one; a mask
  from before the fork or added since stays. The quiet step does not record its masks yet, so a
  fork whose quiet files are missing unmasks nothing. Each change is recorded in `restored.list`
  before it is made, and the engine reads that record, not the script's output, to learn whether
  anything was restored.
- The Linux restore starts the new main once (and stops it again), so a promote of a quiet fork
  runs guest code that a macOS promote does not.
- Not yet run end to end against a live Linux computer pair: the rename itself is covered by the
  unit tests and the live run in the runtime packaging notes, and the promote and undo by the
  engine tests with a fake runtime.

## Review requests: `changes.request`

`changes.request` (a command) records that a fork waits for a review and changes nothing else.
Params: `forkId`, optional `targetId` (default: the computer the fork came from, read from its
fork-time record), `reviewDigest` (from `changes.diff`) and `summary`. The result is
`{request: {id, forkId, target, createdAt, reviewDigest, summary, status}}` with `status: "open"`.
The store is `fold_requests.rs` in the engine, one JSON file per request; a later request for
the same fork and target updates the open one and keeps its id. The CLI and the MCP `fold` tool
(`request: true`) call it after a fresh diff.
