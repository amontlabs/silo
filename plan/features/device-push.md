## Proposed

- One command forks, runs the agent, commits inside the fork, shows the commit on your device for review, and pushes it from there.

## From the old code

The old code had it partly built, about 5,400 lines, and it would be written again.

Lessons:
- Git reads the token from an inherited pipe, never from arguments, environment or files.
- Only committed objects cross; guest hooks and uncommitted files never become host data.
- A stray .DS_Store in the push cache broke every push with a generic error; skip unknown entries.
- The agent can ask for a push but never holds the credential.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/push.rs`
- `app/SiloUI/src-tauri/engine/src/host_push.rs`
- `app/SiloUI/src-tauri/engine/src/host_push_operations.rs`
- `app/SiloUI/src-tauri/engine/src/host_push_transport.rs`
- `docs/SiloUI-GITHUB-IMPLEMENTATION.md#L462`
- `app/SiloUI/src-tauri/engine/src/host_push_cache.rs`
- `docs/SiloUI-RUNTIME-PACKAGING.md#L419`
- `docs/THREAT-MODEL.md#L357`
- `app/SiloUI/src/features/application/components/repository-push-feedback.tsx`
- `app/SiloUI/src/features/application/model/repository-push.ts`
- `app/SiloUI/src-tauri/src/commands/host_push.rs`

Calls: `push.repositories`, `push.start`, `push.status`, `push.dismiss`, `repository.push.start`, `repository.push.status`, `push`, `push.*`.
