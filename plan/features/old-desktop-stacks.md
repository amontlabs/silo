## From the old code

The old code had it and then dropped it, about 200 lines, and it would be written again.

Lessons:
- Several rounds ended with no evidence-based winner; choose by integration evidence, not vendor claims.
- Agents took 'selected a theme' for 'theme applied'; a goal-first skill with an explicit completion test fixed it after 253 benchmark runs.
- Do not build on another vendor's private runtime: a 1.5 GB download, a pinned app pair and a compatibility table.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-DESKTOP-SELECTION.md`
- `docs/archive/SiloUI-DESKTOP-IMPLEMENTATION-PLAN.md`
- `docs/SiloUI-LUDA.md`
- `docs/SiloUI-LUDA-033-DIAGNOSIS.md`
- `docs/SiloUI-LUDA-SKILL-BENCHMARK.md`
- `docs/archive/SiloUI-CHATGPT-APP.md`
- `docs/SiloUI-COMPUTER-USE-PLAN.md#L94`
- `docs/SiloUI-DESKTOP.md#L303`
- `app/SiloUI/api/src/methods/display.rs`
- `app/SiloUI/src-tauri/silod/src/methods/display.rs#L93`

Calls: `display.native.attach`.
