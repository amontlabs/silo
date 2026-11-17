## From the old code

The old code had it built, about 6,300 lines, and it needs reworking.

Lessons:
- Admission guards are bound to the thread that took them, so one dedicated thread must own the whole update transaction.
- An old engine relaunched by the service manager during an install must not resume computers behind the installer.
- Write the resume intent (resumeArmed) before exit, because a restore with --cow-mem resumes the guest by itself.
- Update installation must change the app only, never computer disks; record the running set durably before stopping anything.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/engine/src/engine_update/mod.rs`
- `app/SiloUI/src-tauri/engine/src/engine_update/transaction.rs`
- `app/SiloUI/src-tauri/engine/src/macos_computers/update_save.rs`
- `app/SiloUI/host/src/installer.rs`
- `app/SiloUI/src-tauri/engine/src/runtime/update_save.rs`
- `app/SiloUI/src-tauri/engine/src/runtime/update_recovery.rs`
- `docs/SiloUI-RELEASES.md`

Calls: `engine.update.prepare`, `engine.update.abort`, `engine.shutdown`.
