## From the old code

The old code had it partly built, about 3,300 lines, and it needs reworking.

Lessons:
- A saved configuration journal must block snapshots only while an attempt is live, or one failed attempt freezes the computer list until relaunch.
- Detached runtime children own MicroSandbox's lifecycle guards, not Silo's command lock.
- The lock descriptor was inherited by every process it started, including the VM, so it outlived the engine.
- Test parsers against real launchctl output, not the format a bug report described.
- Holding a global mutex across remote calls deadlocked the app.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/engine/src/runtime/lifecycle_recovery.rs`
- `app/SiloUI/src-tauri/engine/src/runtime/configuration_recovery.rs`
- `docs/direction/evidence/R7.txt`
- `docs/direction/evidence/R10.txt`
- `docs/direction/evidence/F.txt`
- `docs/SiloUI-REVIEW-REMEDIATION-PLAN.md`

Calls: `engine.shutdown`.
