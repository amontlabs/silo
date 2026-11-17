## From the old code

The old code had it built and driven end to end, about 1,200 lines, and its approach can be reused.

Lessons:
- Actions start disabled until the window's listener is ready, so an early menu click cannot be lost.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/src/app_menu.rs`
- `app/SiloUI/src/desktop/app-menu.ts`
- `docs/SiloUI-NATIVE-MENUS.md#L28`
- `app/SiloUI/docs/silo-help.html`
- `docs/SiloUI-NATIVE-MENUS.md#L23`
