## From the old code

The old code had it partly built, about 2,300 lines, and its approach can be reused.

Lessons:
- An install on an allowlist network fails; the error must say the network blocks it (V G1b).
- Install folders must be readable by the working account (Codex /opt/silo/codex was 0700).
- An allowlist network blocks agent install; say which hosts are needed.
- Never report missing for a computer Silo could not ask; that is unknown.
- On macOS, copy the staged file into a root-only folder before checking its hash, so the account cannot swap it.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/agents.rs`
- `app/SiloUI/src-tauri/engine/src/agents.rs`
- `docs/direction/evidence/T5b.txt`
- `docs/direction/evidence/V.txt`
- `app/SiloUI/src-tauri/guest/agent-tools.py`
- `app/SiloUI/src-tauri/guest/agent-tools-lock.json`
- `app/SiloUI/src-tauri/guest/macos/silo-agent-tools.zsh`
- `docs/product/JOURNEYS.md#L24`
- `docs/SiloUI-GUEST-IMAGES.md#L316`
- `app/SiloUI/src/features/agents/components/agents-section.tsx`
- `app/SiloUI/src/features/agents/model/use-computer-agents.ts`
- `app/SiloUI/src-tauri/src/commands/agents.rs`

Calls: `agents.list`, `agents.install`, `silo agents list`, `silo agents install`, `guest.sshAlias.ensure`, `terminal.command`.
