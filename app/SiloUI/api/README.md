# `silo-api`

This is the standalone Rust crate for Silo's canonical engine wire contract.
It owns only serde types and schema generation; it has no dependency on Tauri
or engine behavior.

The crate is a member of the Cargo workspace rooted at `app/SiloUI/Cargo.toml`.
Workspace crates depend on it by path, for example
`silo-api = { path = "../api" }` from `app/SiloUI/client`. Keep the crate's own
manifest so it remains buildable on its own.

The contract source is in `src/`. The checked-in schema is
`schema/silo-api.schema.json` and is regenerated from the Rust types.
