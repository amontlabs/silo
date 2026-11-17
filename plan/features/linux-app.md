## From the old code

The old code had it and then dropped it, about 4,000 lines, and it would be written again.

Lessons:
- GTK suppresses the first release within 500 ms of menu activation, which broke immediate outside-click tests.
- Many Linux-only bugs came from the AppImage environment (editor and terminal launch).
- The APT signing key is kept because existing installs trust it.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/src/linux_menu.rs`
- `docs/SiloUI-NATIVE-MENUS.md#L84`
- `docs/SiloUI-RELEASES.md#L418`
- `docs/SiloUI-DISTRIBUTION-PLAN.md`
- `README.md`
- `docs/SiloUI-LINUX-UPDATES.md`
- `docs/SiloUI-DISTRIBUTION-PLAN.md#L137`
- `app/SiloUI/src-tauri/engine/src/updates/debian.rs`
