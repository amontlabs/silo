# `silo-api`

This is the standalone Rust crate for Silo's canonical engine wire contract.
It owns only serde types and schema generation; it has no dependency on Tauri
or engine behavior.

When the engine workspace is ready, add this crate as a workspace member and
depend on it by path. If `app/SiloUI/src-tauri/Cargo.toml` is the workspace
root, the member path is `../api` and the dependency is
`silo-api = { path = "../api" }`. Keep the crate's own manifest so it remains
buildable on its own.

The contract source is in `src/`. The checked-in schema is
`schema/silo-api.schema.json` and is regenerated from the Rust types.
