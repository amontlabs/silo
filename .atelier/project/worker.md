## Silo
- First run `atelier check --compile-only` once. Its setup steps put the private build file, `node_modules` and the prepared runtime in your worktree. Never print or commit `app/SiloUI/github-build.local.json`.
- Files where parallel work collides most (measured): `app/SiloUI/src-tauri/engine/src/policy/mod.rs`, `docs/SiloUI-POLICY.md`, `docs/THREAT-MODEL.md`, `app/SiloUI/src-tauri/engine/src/secrets.rs`, `app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs`, `app/SiloUI/src-tauri/silod/src/methods/gate.rs` and `app/SiloUI/api/src/methods/registry.rs`. Keep changes there small and additive, and tell the orchestrator when your item must change them.
- After an API change, run both generators from `app/SiloUI` (`cargo run -p silo-api --example generate_requests`, then `--example generate_schema`) and commit their output. The merge regenerates both files, so a conflict only in them is settled there.
- Wire names, saved file formats and checkpoint kinds keep their spelling unless your item says otherwise. Old saved data must still load.
- Never start, stop or connect to a Silo or Silo Dev engine or app, unless your task gives you the Dev engine for a live check: `atelier tmp --short -- app/SiloUI/scripts/live/run <check>`. Only the Dev build and throwaway `e2e-*` computers.
- A user-visible change needs a changeset in `app/SiloUI/.changeset/` (`minor` for a feature, `patch` for a fix).
- Comments describe the code, not the change. User-facing words follow docs/SiloUI-VOCABULARY.md.
- Disk is tight: send long output to a log file and read the summary.
