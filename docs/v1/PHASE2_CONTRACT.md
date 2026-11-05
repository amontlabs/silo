# Phase 2 contract: manifests, diffs, keeps

Shared by the phase 2 slices. Code lives in `app/SiloUI/src-tauri/engine/src/changes/`.

## Manifest format 2

The prototype's format 1 (`git show 6e0fa58c:bench/prototypes/README.md`, `manifest.sh`) plus:

- `META format 2`.
- `GIG repo path size mtime sha256|-`: gitignored files inside a repo, outside pruned trees (`node_modules`, caches, build output), so a changed `.env` shows.
- `DB postgres <db> schema <sha256>` (hash of `pg_dump --schema-only`), `DBT postgres <db> <table> <sha256> <rows>` (per-table data digest), `DB sqlite <path> <sha256>` (file digest, for `*.sqlite`, `*.sqlite3`, `*.db` under allow-listed roots).
- `CRONTXT user <base64>`: the user's crontab text, so a keep merges it by line against the fork-time base. Read by the keep from the stored base manifest; the manifest parser skips it.
- `CRONF user <path>` naming the crontab file, so the diff shows one crontab change, not both the file and the `CRON` line.
- `LAGENT scope label sha256 path` (macOS LaunchAgents/Daemons; scope `user` or `system`).
- `OWN mgr package path`: written by the engine's owner query for changed paths only (dpkg -S, brew, pip RECORD, npm global dir), so files installed by a package are attributed to it.

Escaping as in format 1. One record per line, tab-separated.

## Modules

- `changes::manifest`: `Manifest::parse(&str) -> Result<Manifest>`, typed records.
- `changes::diff`: `diff(base: &Manifest, current: &Manifest, baseline: Option<&Diff>, ignore: &IgnoreList) -> Diff`; `Diff` serializes to the `silo diff --json` shape; `Diff::render(max_lines)` gives the readable text. Buckets: Code, Packages, Settings & files, Database changed, Other changed, Not kept, Ignored (count only).
- `changes::collect`: runs the collector in a guest, streams past `guest.exec` caps, runs the owner query and the database probe, stores manifests per computer (`fork-time` base and later captures), caches the no-op baseline per source checkpoint.
- `changes::keep`: plans and applies a keep (phase 2b).
