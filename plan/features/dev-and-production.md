## From the old code

The old code had it built and driven end to end, about 1,200 lines, and its approach can be reused.

Lessons:
- Only the exact production identifier selects production, so a stray build can never touch production data.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-BUILD-CHANNELS.md`
- `app/SiloUI/host/src/channel.rs`
- `app/SiloUI/scripts/channel_names.rs`
- `app/SiloUI/src-tauri/src/single_instance.rs`
