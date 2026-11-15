# Engine dependency-knot ratchet

Most of `silo-engine` is one dependency cycle: 60 of its 94 top-level modules (321 module-to-module edges) are strongly connected in production code, so a change to any of them rebuilds and retests all of them. The ratchet keeps that knot from growing while it is being taken apart.

## What the check does

`app/SiloUI/scripts/engine_cycle_ratchet.py` runs in the `scripts` step (`app/SiloUI/scripts/ci/scripts.sh`) and takes about 3 s.

- Nodes are the engine's top-level modules, resolved from `lib.rs` including `#[path]` files.
- An edge A -> B is a production reference from A to `crate::B`, `super::…::B` that reaches the crate root, or `silo_engine::B`. Code under `cfg(test)` (test files, inline test modules, attributed blocks) is ignored.
- The knot is the largest strongly connected component.
- `app/SiloUI/scripts/engine-cycle-baseline.json` lists the knot's modules and the edges between them. The check fails when the knot gains a module or an edge, naming the edge and its source lines. A new edge outside the knot that does not close a cycle passes.
- When the knot shrinks the check also fails, with the command that lowers the baseline: `python3 app/SiloUI/scripts/engine_cycle_ratchet.py --update`. `--update` refuses to write a baseline that is larger than the committed one.
- It prints the change's reach: the share of engine modules and lines that transitively depend on the modules the diff touches (diff against the merge base with `main`, or `--base <ref>`).

Tests: `app/SiloUI/scripts/test_engine_cycle_ratchet.py` (a planted cyclic edge fails, the committed baseline matches the engine).

## Existing tools

No established Rust tool offers a committed baseline that may only shrink.

- `cargo-modules` can print a module graph and detect cycles but has no baseline or diff failure.
- `cargo-depgraph` and `cargo tree` work at crate level, where the engine has no cycle.
- `cargo-deny` and `cargo-machete` check crates and unused dependencies, not module edges.
- `rustc` allows module cycles inside a crate.

The scanner is a text tokenizer, not rustc: it does not see edges created by macro expansion or glob re-exports from the crate root, and it misses a reference spelled only through a root `pub use` of another module's items. Moving to crates (one per module) would make Cargo enforce acyclicity and retire this script.
