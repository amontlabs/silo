# Phase 2 contract: manifests, diffs, keeps

Shared by the phase 2 slices. Code lives in `app/SiloUI/src-tauri/engine/src/changes/`.

## Manifest format 2

The prototype's format 1 (`git show 6e0fa58c:bench/prototypes/README.md`, `manifest.sh`) plus:

- `META format 2`.
- `GIG repo path size mtime sha256|-`: gitignored files inside a repo, outside pruned trees (`node_modules`, caches, build output), so a changed `.env` shows.
- `DB postgres <db> schema <sha256>` (hash of `pg_dump --schema-only`), `DBT postgres <db> <table> <sha256> <rows>` (per-table data digest), `DB sqlite <path> <sha256>` (file digest, for `*.sqlite`, `*.sqlite3`, `*.db` under allow-listed roots).
- `CRONF user <path>` naming the crontab file, so the diff shows one crontab change, not both the file and the `CRON` line. `CRON user <sha256>` hashes the file's exact content, read as the collector's account (root on Linux); `CRON user -` is no crontab and `CRON user ?` is a crontab that exists and could not be read, which the diff never compares. `CRONT user <text>` is the crontab text when it is at most 64 KiB, for line counts and keep.
- `QPARK path sha256|-`: a file the quiet step set aside under `/var/lib/silo-quiet/disabled`, listed under its original path (crontabs, timers, autostart entries, rc links). `QCRON user sha256 text`: a parked crontab in full. A quiet fork's base has no crontab because it was parked, so the diff and keep's three-way merge use the `QCRON` original as the base: a fork that adds one line shows `cron root: 1 line added`, and one that leaves it parked shows nothing.
- `DBU postgres reason`: Postgres is installed but could not be read when the manifest was captured (no answer, cannot act as the postgres account, incomplete tools). The collector never starts a service. A database that appears on the readable side is then `not compared`, not new, dropped or changed.
- `LISTEN proto port process`: listeners are read from `/proc/net/tcp` and matched to the process holding the socket (as root every process is visible); `ss` and `lsof` are used only where there is no `/proc`.
- `LAGENT scope label sha256 path` (macOS LaunchAgents/Daemons; scope `user` or `system`).
- `OWN mgr package path`: written by the engine's owner query for changed paths only (dpkg -S, brew, pip RECORD, npm global dir), so files installed by a package are attributed to it.

Escaping as in format 1. One record per line, tab-separated.

## Modules

- `changes::manifest`: `Manifest::parse(&str) -> Result<Manifest>`, typed records.
- `changes::diff`: `diff(base: &Manifest, current: &Manifest, baseline: Option<&Diff>, ignore: &IgnoreList) -> Diff`; `Diff` serializes to the `silo diff --json` shape; `Diff::render(max_lines)` gives the readable text. Buckets: Code, Packages, Settings & files, Database changed, Other changed, Not kept, Ignored (count only).
- `changes::collect`: runs the collector in a guest, streams past `guest.exec` caps, runs the owner query and the database probe, stores manifests per computer (`fork-time` base and later captures), caches the no-op baseline per source checkpoint. The baseline is measured by `silo diff --baseline` on a throwaway fork of the same checkpoint (see `SiloUI-CHANGES.md`); `changes.diff` also returns `base_source` and `elapsed_seconds` for it. `changes.baseline` run on a fork returns no baseline rather than one that contains the task's changes: it refuses when the fork ran more than two minutes past its idle time or moved a repository or package.
- `changes::keep`: plans and applies a keep (phase 2b).
