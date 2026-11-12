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
| Not kept | What a merge never brings over: new listening ports, processes, services, and credential paths (`~/.ssh/`, `~/.aws/credentials`, `~/.config/gh/hosts.yml`, `~/.openclaw/credentials/`, `*.pem`, keychains; the list is in `changes/ignore.rs`). |
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
- Every entry a keep can bring over or conflict on carries an `id` (see
  [Item ids](#item-ids)): `settings.files[]`, `other[]`, `code[].gitignored[]`, `code[]`
  (the repository), `settings.cron[]`, `settings.launch_agents[]` and `database[]`. Each
  package a keep acts on is listed in `packages[].ids`, and each uncommitted file of a
  repository in `code[].dirty_ids` (repo-relative path to id). The ids are left out of the
  review digest, so adding them did not change any digest.

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

## Item ids

An item is one file, one package of a manager, one repository, one crontab, one launch agent or
one database. Its id is built only from what names it, so it is the same in the diff, in a
conflict and in a choice, and does not depend on the order of anything:

| Item | Id |
| --- | --- |
| a file (also an uncommitted file in a repository) | `file:<absolute path>` |
| a package | `package:<manager>:<name>` |
| a repository | `repo:<absolute path>` |
| a crontab | `cron:<user>` |
| a launch agent or daemon | `launch-agent:<scope>:<label>` |
| a database | `database:<engine>:<name>` |

## Merging a fork: `silo merge` and `silo undo`

`silo merge <fork> [--into C] [--choose ITEM=ANSWER]... [--skip ITEM]... [--dry-run] [--reviewed DIGEST] [--data-ok] [--json]` merges a
fork's changes into the computer it was made from (the engine method is `changes.keep`, in
`app/SiloUI/src-tauri/engine/src/changes/keep/`). `silo undo <computer>` puts the computer back
to before its latest successful merge (`changes.undo`). Both are MCP tools (`merge`, `undo`).
`silo fold` is a hidden alias that prints one line saying it is now `silo merge`.

A merge builds its plan from three manifests (the fork-time base, the fork, the target); the
diff supplies the item list (never Not kept or Ignored) and the database check. Files are
compared live: both computers are hashed at merge time, so a file the collector could not hash,
an unreadable file or a changed mode is never taken as unchanged.

1. **Pinned to what was reviewed.** `silo diff --json` returns a `reviewDigest`; `--reviewed`
   passes it back, and the merge refuses (`changed`) if the fork's changes differ. The digest is a
   SHA-256 over the reviewed content only: the Code, Packages, Settings & files, Database and
   Other changed entries as shown after the ignore list and the baseline, with their identities
   (hash, size, link target). It leaves out the Ignored count, idle noise, Not kept, and every
   timestamp (an entry carries no modification time). `silo merge --dry-run` does not send a digest, so
   it never refuses on a stale one: a dry run reports and does not pin. Whatever the
   merge writes is read from the fork once, before main is touched, and the fork is looked at
   again afterwards: if it moved meanwhile, the merge stops.
2. **Checkpoint, then lane.** A `before-keep` checkpoint named `Before keep of <fork>` (saved names keep their spelling) of the
   target (on both platforms, with the same retention count; a running computer is captured
   with its memory, a Linux one with `--full --guest-flush required`), then the target's operation lane is held
   until the apply ends, so no restore, stop, fork or delete of it interleaves. The target is
   captured after the lane is taken. Every check below that does not change the target runs once
   before the checkpoint too, so a refused merge is quick and creates nothing; the checks run
   again once the lane is held. On Linux a merge reads and writes as root, and runs repository
   commands and recipe steps as the owner of the folder they work in.
3. **Checks, before anything is written.** Per file, package, crontab line and uncommitted file:
   if the target changed it since the fork point and the fork ended differently, it is a
   conflict (uncommitted files are compared with the blob at the fork point, so a commit on
   main counts). Git history is searched for credentials, `.env` files and ignored paths; any
   found refuses the merge (`credentials`) with the list. The fork's commits are fetched into a
   temporary ref and tested with `git merge-tree`; a conflict, or a git that cannot test, is a
   conflict of that repository. Destinations are checked: a link or non-folder in the path, or
   a folder where a file goes, is a conflict. Package names are validated; brew installs only
   when it would install the fork's version. A database change is carried only with a
   `keep: migrate:` step in the recipe of the computer being merged into and changed migration
   files (folders named `migrations`, `migrate` or `alembic`), or `--data-ok`; otherwise the
   refusal points to `silo replace-main`. A refusal is a
   result (`status: "refused"`, exit code 2, or 3 when it only needs answers) and the target is untouched. A conflict is not a
   dead end: see [Conflicts are questions](#conflicts-are-questions).
4. **Apply, in order.** Package installs and removals (each manager as the user it belongs to),
   git merge, uncommitted files and deletes (renames delete the old path), files (written next
   to the target and renamed; links recreated, never read through), crontabs merged by line
   against the fork-time text (`CRONTXT` in the base manifest), launch agents (a plist that
   will not load fails the merge), then the `migrate:`, `restart:` (the process id must
   change and the new process must run) and `after:` hooks. They come from the recipe the
   target computer was built from, which the engine reads itself: hooks a caller sends are
   ignored for a computer built from a recipe, and used for one built from none
   ([recipes](SiloUI-RECIPES.md#what-a-computer-records)).
5. **Failure and crashes.** A failing step restores the checkpoint, which then can no longer be
   undone. The journal (synced to disk) lets the next merge, `silo undo`, or the engine's start
   restore the checkpoint of an interrupted merge.
6. **One merge at a time per target.** A second waits up to two minutes, then fails with `busy`.

`silo undo` restores the latest checkpoint of a merge that succeeded, once, and lists what
changed on the computer after the merge (that is discarded). A restore leaves the computer
stopped, so the undo then starts it when it was running at the merge (the checkpoint has memory):
the start resumes the checkpoint's memory, processes and open databases as they were. A
computer that was stopped stays stopped. The result says which (`main`, `resumedFrom`) and
carries a note when main was put back but could not be started. A restore has no disk-only
fallback for a checkpoint with memory. Temporary bundles and git refs are
removed on every path.

### Conflicts are questions

An item both sides changed is a conflict. The merge does not stop on it for good: the caller
answers each one and the merge goes on. The call takes `choices` (item id to answer) and `skip`
(item ids). On the command line they are `--choose <id>=<answer>` and `--skip <id>`, repeated
for each item; the MCP `merge` tool takes `choices` as an object and `skip` as a list:

| Answer | Allowed for | What it does |
| --- | --- | --- |
| `fork` | files, packages, crontabs, launch agents | The fork's version is written over main's (a package is installed at the fork's version or removed; a crontab is replaced by the fork's whole crontab). |
| `main` | everything that can conflict | The item stays as main has it. |
| `branch` | repositories | The fork's commits are brought into main's repository as a branch named `silo/<fork name>`. Nothing is merged, checked out or reset, and the working tree is untouched, so Git can resolve it later. |

- A repository both sides changed offers only `branch` and `main`: Silo does not force-reset
  someone's repository. `fork` is accepted for a repository that merges cleanly (it is then the
  ordinary merge) and is an error, naming the repository, for one that does not. An existing
  branch is never moved: if `silo/<fork name>` exists at other commits, the next free name
  (`silo/<fork name>-2`, ...) is used, and if it already holds the fork's commits nothing is
  made. Uncommitted files of a repository whose commits go to a branch are not carried; the
  report says so.
- `skip` lists items not to bring over even though they do not conflict. It is the same as the
  answer `main`; the only difference is the word in the report (`skipped` instead of
  `left-as-main`). An id in both `skip` and `choices` must be answered `main`.
- A choice for an id that is not in the reviewed diff, or an answer the item does not allow
  (`branch` for anything but a repository with commits, any answer for a database), is an
  error that names the item, before anything is fetched or changed.
- **Without an answer** the merge is refused with `reason: "choices-needed"` (still
  `status: "refused"`; the command exits with 3, where any other refusal exits with 2) and
  `conflicts` lists every unanswered conflict: its `id`, the `item` and `detail`, `forkChange`
  and `mainChange` (what changed on each side, in a few words) and `allowed`. The command prints
  each one with its id and the answers it allows. Run the merge again with an answer for each
  (`--choose <id>=<fork|main|branch>`), or leave an item as main has it with `--skip <id>`. A destination that cannot be written, an
  unreadable file and a package version brew cannot install allow only `main`. The old reasons
  `divergent` and `git` are gone.
- **Credentials stay a wall.** They are never brought over and take no answer. A commit or an
  uncommitted file with credentials, a `.env` or an ignored path refuses the merge
  (`credentials`), whatever the answers, including `branch`. Their conflicts list no allowed
  answers and cannot be named in `choices`.
- **Databases are never combined.** A database takes no answer. As before, a database change
  needs a `keep: migrate:` step in the target's recipe with changed migration files, or `data_ok`, and the report
  says what happened to each one (`not-combined`): that main's database was left as it is, and
  whether the recipe's migrate step runs on it.

**Choices and the review digest.** The digest pins what the fork changed, and choices are
checked against that same diff but are not part of the digest. They answer how those changes
meet main, which moves on its own, and a digest that covered them would have to change every time
the answers did. A merge reaches the check only after its digest matched (or none was sent), so
the diff in hand is the reviewed one; a dry run checks the fork as it is now. Both then run the
same planning with the same choices, so `--dry-run` with choices reports what the real run does:
the same `items` and `summary`. A review request (`changes.requests.keep`, `silo requests merge <id>`) passes `choices` and
`skip` through the same way.

**The report** gains `items`, one per item, in id order, each with its `id`, `kind`, `label`,
`outcome` and `detail`. The outcome is `brought-over`, `left-as-main` (answered `main`),
`branch` (a repository's commits left as a branch), `skipped` (in `skip`), `already-there`
(main already holds it) or `not-combined` (a database). It also gains `fastForward`: `true`
when main was unchanged since the fork was made, from the fork-time record compared with
main's capture the same way the fork is (databases do not count, since they are never
combined). Then nothing could conflict, every change is applied, and main keeps running: the
computers are not swapped, because the agent lives in main. The plan is the same either way.

**Undo** is unchanged. It restores the before-keep checkpoint, which holds the whole disk, so
a branch the merge made goes with everything else the merge wrote.

## Replacing main with a fork: `silo replace-main <fork>`

`silo replace-main <fork> [--into main] [--dry-run] [--json]` (MCP `replace_main`; `silo fold --promote` still works as a hidden alias) makes
the fork the computer a merge would have changed (`changes.promote`,
`app/SiloUI/src-tauri/engine/src/changes/promote/`). The fork takes main's name and settings;
main is archived as `main-before-<yyyymmdd-hhmm>` (a number is added if taken), stopped, with
its checkpoints. `silo undo main` (or `silo undo <fork>`) swaps everything back. Replacing main restarts main, so
whatever runs there ends, and asked from inside a computer it always asks the owner first.

Replacing main refuses, changing nothing, when either computer is busy, the two run different
systems, they carry the same name, or (Linux) one of them keeps a disk in a folder named after it. Otherwise it saves a
`Before promote of <fork>` checkpoint of main (saved names keep their spelling), stops both, holds both operation lanes (in id
order), writes the journal (`changes/promote/<fork id>.<main id>.promote.json`, synced) and
applies these steps, recording each:

| Step | What moves |
| --- | --- |
| names | the fork becomes `main`, old main becomes the archive name (computer record on macOS). A rename that would give two computers one name fails, so a duplicate `main` cannot exist even between the two renames. Linux: `msb rename` of the stopped main (to the archive name) and then of the stopped fork (to `main`), then both saved names in one write. Each sandbox is identified by the `silo.machine-id` label it carries, never by its name, so a repeat after a crash renames only what is not renamed yet; the SSH entries are rewritten here too (below) |
| name-keyed settings | secret assignments (`secrets.rs`) and the GitHub assignment (`github.rs`) are keyed by name, so main's stay with the name `main`, now the fork's; the fork's own grants under its old name are dropped like a removed computer's (saved choices, cached attachments and issued tokens), never added to main's. Published ports move from main's id to the fork's on macOS (`macos-computers/ports`, keyed by id); on Linux they are saved under the computer name (`network.json`), so main's stay with the name `main`, now the fork's, and the fork's own are removed (an undo gives them back); URL routes (`url_proxy`, hosts derived from the name) are forgotten and rebuilt from the new name |
| SSH access | the saved SSH entries are named after the computer id (`silo-<id>`), so main's entry is rewritten to connect to the new main (with its key and pinned host key) and the archive's to the archive; only entries that exist are touched. Linux entries connect by computer name and pin that sandbox's host key, so they are rewritten at the end of the names step, after both sandboxes carry their final names (an undo rewrites them back the same way) |
| lineage | read from the journal, nothing is rewritten: while a replace is in force, forks made from the old main resolve to the new main (so a merge of those forks targets it), and the new main's own source is the old main's. An undo ends the record and every answer returns |
| macOS network identity | each computer keeps its own proxy CA (the guest trusts the CA it was set up with and the start verifies it against the saved network identity, so the CA stays with its disk) and the new main takes main's hostname, written to the stopped disk's system configuration; the archive takes the fork's hostname. The machine identifier stays with each disk: Apple ties a guest's hardware identity and what it activated to the identifier it first booted with, and the disks swap names, not hardware |
| quiet parking | macOS: the scheduled work parked when the fork was made quiet is put back from `Library/Application Support/Silo/Quieted/` on the stopped disk, except at paths the fork has used since (those stay parked and listed). Linux: the new main is started, the cron, at, anacron and systemd timer items under `/var/lib/silo-quiet/disabled` are moved back through the guest (`quiet_restore.sh`, as root) except at paths the fork has used since, and it is stopped again; a timer the quiet step only masked is unmasked, and `/etc/crontab` gets its parked text back when it still holds nothing but comments. Items parked for the services the fork was asked to stop stay parked. Best effort; it never fails a promote |

Keyed by id and therefore untouched: changes store manifests, checkpoint records and the
proxy address slot.

`silo undo` takes back the newest of a replace and a merge of that computer (an interrupted
merge first). Undoing a replace:

- reverts only the latest replace that involves either computer; an older one is refused with
  "Undo that one first" while a later replace built on it;
- refuses, changing nothing, when the two no longer carry the names the replace gave them, or
  when another computer took the fork's old name, so two computers are never named `main`;
- puts back what the replace moved and keeps what happened to the main role since: ports
  changed on the new main while it was main go back to the old main with the name, and the
  fork gets the ports it had. Secrets and GitHub settings are keyed by name and follow the
  name. Not restored exactly: a secret value or GitHub token issued while promoted lives in the
  Keychain under the name and stays with it; the fork gets its recorded assignments back, and
  its GitHub runtime identity is created at its next start.

A crash is repaired when the engine starts: a replace with every step but quiet parking
recorded is rolled forward, any other is rolled back (steps are idempotent and reverted in
reverse order); an interrupted undo is finished. Both computers are held (their operation
lanes) from engine start until their recovery is done, and the recovery retries while the
runtime is not ready, so a half-swapped computer cannot start. The next replace or undo of
either computer settles it first.

Linux computers are replaced with the same steps. The runtime names each sandbox after its
computer, so the names step uses the stopped-sandbox rename of the bundled `msb`
(`microsandbox-rename-stopped`, see [runtime packaging](SiloUI-RUNTIME-PACKAGING.md#rename-a-stopped-sandbox-2026-11-05)).
Checkpoint records, SSH aliases' file names and changes manifests are keyed by computer id and
need no change; the snapshot group is stored on the computer's record and keeps its name.

Known gaps:

- A Linux computer whose configuration still mounts a disk from `volumes/<name>/` (not yet
  converted to a disk the computer owns) is refused with `storage`: the folder is named after the
  computer and a delete by name would remove the wrong one.
- Linux quiet parking is restored only for scheduled jobs. A masked timer is unmasked only when
  the quiet step listed it in `<token>.masked-timers.list` (written before each mask it makes); a
  mask from before the fork or added since stays, and a fork with no list unmasks nothing. Each
  change is recorded in `restored.list` before it is made, and the engine reads that record, not
  the script's output, to learn whether anything was restored.
- The Linux restore starts the new main once (and stops it again), so replacing main with a quiet fork
  runs guest code that a macOS replace does not.
- Not yet run end to end against a live Linux computer pair: the rename itself is covered by the
  unit tests and the live run in the runtime packaging notes, and the replace and undo by the
  engine tests with a fake runtime.

## Review requests: `changes.request`

`changes.request` (a command) records that a fork waits for a review and changes nothing else.
Params: `forkId`, optional `targetId` (default: the computer the fork came from, read from its
fork-time record), `reviewDigest` (from `changes.diff`) and `summary`. The result is
`{request: {id, forkId, target, createdAt, reviewDigest, summary, status}}` with `status: "open"`.
The store is `fold_requests.rs` in the engine, one JSON file per request; a later request for
the same fork and target updates the open one and keeps its id. The CLI and the MCP `merge` tool
(`request: true`) call it after a fresh diff, and pass `counts` (the number of changes in each
bucket) so a list can show them without a diff.

A request is `open`, `keeping` (a keep of it started and is not recorded as finished), `kept`
(with `keepId`, the target's before-keep checkpoint, and `resolvedAt`) or `thrown-away`. It is
also `thrown-away`, with `closedReason` `fork-deleted` or `target-deleted`, when its fork or
target computer is deleted: `computers.delete` closes the open requests that name the computer, and
`changes.requests.list` closes any open request whose fork or target is missing, but only when
both platforms were read and the Linux runtime reported no repair (never from a partial list). A
`keeping` request is left to its keep. A request
is made only for a fork of its target: `changes.request` refuses a fork whose fork-time record does
not name the target (so main and unrelated computers cannot be named), and a folder target needs
the folder record the CLI keeps for that fork. A fork's source may be recorded as a checkpoint
(no origin was stored); it counts as the computer that owns the checkpoint. The CLI writes the
folder record into its channel's state directory (`~/.silo` or `~/.silo-dev`), where the engine
reads it, even when it was given another socket. A keep made without the request
(`changes.keep`) marks the open requests for that fork and target kept. A folder merge by the CLI
does the same through `changes.requests.markKept` (`forkId`, `target: "folder:<path>"`, optional
`keepId`, the folder's backup ref); the engine checks the fork is the folder's fork first, and a
failure to close the request is a note in the merge's result, not a failed merge.

Five more methods work on one: `list` takes an optional `status` (`open` includes `keeping`);
`show`, `keep`, `throwAway` and `open` take `requestId`. `keep` accepts an open or `keeping`
request, `throwAway` and `open` only an open one; `show` also reads a kept or thrown-away request.
One job at a time works on a request: a second keep or throw away of it is refused while the first
runs. A keep marks the request `keeping` before it starts. When the engine stops between the keep
and the mark, the next keep of the request (or the next `list`) finds the finished keep in the keep
journal (the same fork, target and digest, succeeded after the keep started) and marks the request
kept instead of keeping again. A keep that was undone since still counts as kept: changes the user
undid are not applied again for the same request (ask for a new review). A keep that was rolled
back, or never got going, with none pending for the target, makes the request `open` again. A new
`changes.request` for a fork whose keep is `keeping` is refused until it is settled.

- `changes.requests.list` (a query): `status` is `open` (default), `kept`, `thrown-away` or `all`.
  Each entry is the request with `forkName`, `forkState` (absent when the fork is gone) and
  `targetName`.
- `changes.requests.show` (a job): the request, a fresh `changes.diff` of the fork (the last
  capture when the fork is stopped, with `fresh: false`), `reviewDigest` (now), `stale`
  (`true` when it differs from the request's digest), `counts` per bucket, readable `text` and
  `credentials: {used: [{secret, host, method, count}], blocked: [{host, method, count, reason}]}`.
  The credentials come from `fold_requests::CredentialSource`; until the proxy installs one both
  lists are empty.
- `changes.requests.keep` (a job; optional `dataOk` and the recipe's `keep` steps): runs the
  `changes.keep` work for the fork and the request's target, pinned to the request's
  `reviewDigest`, so a fork that changed since is refused (`status: "refused"`, reason `changed`,
  the request stays open). A kept result marks the request `kept` with the checkpoint as `keepId`
  and returns the keep's result with the updated `request`. A request without a digest is refused
  (there is nothing to pin the keep to), as is a request whose fork is gone.
- `changes.requests.throwAway` (a job): stops the fork if it runs and deletes it the way
  `computers.delete` does, then marks the request `thrown-away`. Before it drops anything it
  checks again that the fork is a fork of the request's target, so it never drops main, the target
  or an unrelated computer. A request whose fork is gone is only closed, and only when the list of
  computers is complete (both platforms read, no runtime repair pending).
- `changes.requests.open` (a job): starts the fork when it is `stopped` (`computers.start`), waits
  up to two minutes for it to report `running`, and returns `computerId`, `name`, `state`,
  `started`, `sshAlias` (`<name>.silo`, or `.silo-dev` from the development build; it works after
  `silo ssh-setup`) and `urls` from the URL proxy.

Folder forks (`target: "folder:<path>"`): only the CLI reads the folder's git history, so the
engine's `changes.requests.keep` refuses them with "Merge folder forks with `silo merge`", and
`changes.requests.show` returns `diff: null`, the request's own summary as `text` and
`stale: null`; `silo requests show` reads the folder itself and fills the text, digest and `stale`.
`throwAway` and `open` work for them like for any fork.

Merges a computer's agent asks for. A `changes.keep` from a computer into itself, or its
`changes.requests.keep`, is held for the owner as an approval of kind `merge`
([SiloUI-POLICY.md](SiloUI-POLICY.md#asking-you)). A `changes.keep` must carry the `reviewDigest`
of a `changes.diff` the agent ran (without it the call is refused and the agent is told to run
the diff first; a dry run is refused too). Asking makes or reuses the open review request for that
fork and target, with that digest and the engine's summary, and links the two: the request's
`approvalId` and the approval's `reviewRequestId` and `reviewDigest`. Asking again with other
changes updates the same request's digest and withdraws the earlier approval. Once a request is
linked to an approval, only the engine writes its text: its summary is the approval's, its counts
are empty, and a later `changes.request` for the same fork (by the agent, with the same digest or
another) changes neither. Allowing the approval runs
`changes.requests.keep` for the request with the stored `choices`, `skip`, `keep` and `dataOk`,
only when the request is still open and still has the approval's digest. Keeping the request
directly (`changes.requests.keep` from the app or the command line) allows the approval with the
keep's job; throwing it away denies it; a request closed any other way (its fork or target was
deleted, or the agent threw it away) withdraws it the next time approvals are read.

## Steps and the fork tree

A step is one command run on a computer through Silo: `silo exec`, the MCP `exec` tool, or the
command of `silo fork ... -- <cmd>` (the folder-fork command is not recorded).

**Choice.** Recording is a separate engine command, `steps.record`, called by the CLI and MCP
server after the command (and the checkpoint, when one is taken) finishes. The alternative,
putting step fields on `checkpoints.create`, would have made steps exist only when step
checkpoints are on, and would have tied one record to one job. A step is recorded whether or
not a checkpoint is taken; a failure to record never fails the command (the result carries
`step.error`).

- `steps.record` (command): `computerId`, `command` (argv), `exitCode`, `startedAt`,
  `finishedAt`, optional `checkpointId`. The engine numbers the step (1-based per computer,
  never reused, also after old records are dropped) and returns `{step}`: `id` (`st-` and 8
  hex), `computerId`, `number`, `command` (argv joined, arguments with spaces single-quoted,
  cut to 200 characters, ending in an ellipsis), `exitCode`, `startedAt`, `finishedAt`,
  `checkpointId`, `parentStep` (the previous step's number) and, for a fork's first step,
  `parentCheckpoint` (the checkpoint the fork was made from, from its fork-time record).
- `steps.list` (query): `computerId` to `{steps}`, oldest first. The records are one JSON file
  per computer under `<engine data>/steps/` (`steps.rs`); the newest 500 are kept and the file
  goes with the computer. Command lines are stored as typed, so keep secrets out of them.
- `silo steps <computer>` prints a table (`--json` the same data); the MCP tool is `steps_list`.

**Step checkpoints.** `silo exec` and `silo fork ... -- <cmd>` take `--step-checkpoints`
(default off) and `--keep-steps N` (default 20; 0 keeps every one); the MCP `exec` and `fork`
tools take the same. When on, the CLI saves a checkpoint of the computer after each step with the existing
`checkpoints.create` job, kind `step` and `keepNewest` set to the keep count. `step` is a
checkpoint kind like `automatic` and `before-keep` (`silo checkpoint --kind step` makes one by
hand): after each step capture the engine runs the same retention pass as its own job, keeping
the newest N of the computer's `step` checkpoints and never touching other kinds, the newest
checkpoint of any kind, or one a fork or another operation builds on (so the checkpoint a fork
was made from stays). A removed checkpoint stays in the step list.

**Fork from a step.** `silo fork <computer> --from-step N` (MCP `fork` with `fromStep`) looks up
the step and forks from its `checkpointId` (`checkpoints.fork` with that checkpoint). It stops
before forking when there is no step N, when the step has no checkpoint (step checkpoints were
off), or when the checkpoint was removed by retention.

**Fork tree.** `computers.tree` (query, no parameters) returns `{nodes}` with `id`, `name`,
`parent` (a computer id or null), `from`, `state` and `pendingChanges`, parents before their
children. `from` is `{kind: "root"}`, `main` (the parent's state when the fork was made, from a
fork-point or warm checkpoint), `checkpoint` (`checkpointId`), `step` (`number`,
`checkpointId`), `recipe` (`hash`) or `folder` (`path`; the CLI fills this from its folder
record, the engine does not know folders). Parent and origin come from the fork-time record
the engine already keeps (the changes store: the checkpoint and computer noted when the fork
was created and kept in the base record once it starts), read through the promote journal
(`lineage.rs`): after `silo replace-main` the promoted fork is a root, or has the old main's own
parent, and the old main's forks hang under it; `silo undo main` ends the record and every
answer goes back. `pendingChanges` is `{requestId, buckets}`: the open review request of the
fork, and `buckets` is always null for now (counting needs a capture). The hidden throwaways
(`silo-baseline-*`, `silo-build-*`) are left out. `silo tree` prints it indented with state, origin and
pending review; `--json` is the same data; the MCP tool is `tree`. Computers on other devices
are not included.

## Recipe additions and pull requests

- **Recipe additions.** After a merge brings packages or settings files, the engine appends them to
  the recipe the target computer was built from (the computer's record, and the owner's stored
  recipe of the same name; a recipe that ships with Silo is never edited). The merge result lists
  what was added. Packages are the diff's top-level added packages per manager (`dpkg` is `apt`,
  `npm -g` is `npm`); dependencies are not listed; a manager recipes have no list for (`brew
  cask`) is left out. Nothing is appended for a dry run, a refusal or a folder fork. See
  [Recipes](SiloUI-RECIPES.md#what-a-computer-remembers).
- **Pull request.** `silo merge <fork> --pr` reads the fork and changes nothing in it except
  committing its uncommitted work, which a folder merge does as well. It is a device-only command;
  [the CLI reference](SiloUI-CLI.md#pull-requests) says how it works.

## Credentials used: `computers.credentials`

`computers.credentials` (a query; params `computerId`, optional `since` in epoch milliseconds)
returns `{used, blocked}` read from the proxy's `secret-usage.jsonl` (see
[secrets](SiloUI-SECRETS.md#credentials-a-fork-used)). `silo status` and `silo diff` print the
`credentials used:` line and carry `credentialsUsed` in `--json`; the value is `null` when an
older engine does not know the method. The Linux events come from the
`microsandbox-secret-usage-events` patch, the macOS ones from the same patched network crate
inside `silo-macos-gateway`. `changes.requests.show` fills its `credentials` field from the same
files: silod installs that source at startup.
