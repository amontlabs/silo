## Proposed, not confirmed by the owner

- The engine is state, calls and events. State is a set of records. A call asks for a change. An event says what happened. The app shows state and follows events.
- One name per concept everywhere: a part of the plan, a group of calls, one engine file, a command, a screen.

## From the old code

The old code had it built, about 6,500 lines, and its approach can be reused.

Lessons:
- 190 calls in 37 groups grew partly because app screens got their own calls (display.viewer.*, exports.dismiss); a rewrite should keep calls per user action.
- Events are not yet cut per computer, so a computer's agent cannot subscribe and has to poll.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/registry.rs`
- `app/SiloUI/api/src/methods/request.rs`
- `app/SiloUI/api/schema/silo-api.schema.json`
- `app/SiloUI/api/README.md`
- `app/SiloUI/src-tauri/engine/src/events/mod.rs`
- `docs/ROADMAP.md#L414`
- `docs/ROADMAP.md#L456`

Calls: `events.subscribe`.
