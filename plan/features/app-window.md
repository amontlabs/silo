## Proposed

- Zig client inside the app
  - Rule: The part of the app that speaks the engine's calls and events and keeps the current state is Zig, linked through a C interface as Ghostty links its core.
  - Rule: Swift reaches the engine only through that client, and the core exports no extra functions for the app.
  - Rule: The app holds one store fed by the Zig client, and views are a function of that store, with no state of their own and no knowledge of the transport.
  - Rule: Calls across the Swift and Zig boundary are few and coarse, with one batch of changes per update and never one call per record.
  - Rule: Everything the engine hands to the Swift side is bounded in size, count and length.
  - Linking the Zig client into the app takes a hand-written C header, a static library, an xcframework and a module map, with exported functions limited to C types.
  - The app needs no universal binary, because Silo targets Apple Silicon only.
  - Rule: Swift code that parses bytes never indexes `Data` from zero after removing a prefix; it uses `startIndex` or copies the bytes, and a test covers it.
- State on screen
  - Property: The app never shows a state the engine never had.
  - Property: Nothing is shown as running unless its heartbeat is fresh; otherwise it is shown as stale.
  - Rule: While a call is pending, the app shows the safer state, such as access not yet granted or a fork not yet merged.
  - Property: A reply or an event about a computer, a fork or a screen that no longer exists is dropped and never applied.
  - Rule: A status view that cannot update promptly is not shipped.
- No waiting
  - Property: The app never blocks on the engine or a computer; every action is acknowledged at once and shown as in progress.
  - Rule: Each view arrives complete in one response from the client; no view is assembled from a chain of requests.
  - Rule: When an event arrives, the app invalidates only what the event names, never a whole screen.
  - Rule: The app shows no spinner where the state is already local.
- In the window
  - Rule: The app is designed for about three things needing the user at once, whatever the number of forks.
  - Each computer in the list has its failure reason or its latest log line beside it.
  - When a failure or an approval needs to be seen, the app opens the page that shows it without being asked.
  - Running work can be stopped or cancelled from any view that lists it.
  - The app has back and forward navigation between views.
  - Forks can be pinned in the app.
  - An agent session offers to copy the command that attaches to it from a terminal.
- Tests
  - Test: A simulator and app test navigates back and forth fast, leaves and returns during a fork or a merge, and drops a fork with a call in flight; no stale or doubled action remains.
  - Test: A check on the Zig client and the Swift app shows that view code can change engine state only by sending a call.
  - Test: The app is load-tested early, with many computers and sessions open; it stays responsive and does not crash.
  - Test: The boundary between the Zig client and Swift is benchmarked inside the real Swift app, not in a separate test host.
  - Test: Every view is checked with the window shrunk to less than half the screen; tables and layouts do not break.
  - Test: A test opens a computer or a folder that holds old state; the app does not hang.
  - Test: Each release measures the app's idle CPU use and publishes the figure.

## Open

- Does the app open on the list of computers as planned, on only the decisions waiting for you, or on an agent session?
  - Suggested: The list of computers with what waits on you at the top; nothing waits in the shell stage, and opening on a chat hides the rest of the app.
- Does the app add an overview level between the list and a computer's page, showing what the computer is doing with an approval to answer in place?
  - Suggested: Yes, added in the agent stage; until there is an approval to answer, the list and the page are enough.
- Is the app one window for every computer as planned, a window or tab set per fork, or a main view with an inspector window beside it?
  - Suggested: One window, because the app must show many forks in one view and each extra window is one more place to look.

## From the old code

The old code had it built, about 6,800 lines, and it needs reworking.

Lessons:
- A failed start leaves the computer Stopped; keep the result of the request separate from the computer's state, keyed by computer id.
- Names and disk sizes are fixed after creation; only CPU and memory can change.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src/features/application/components/application-shell.tsx`
- `app/SiloUI/src/features/application/application-app.tsx`
- `app/SiloUI/src/features/application/model/use-application-navigation.ts`
- `app/SiloUI/src/features/application/pages/overview-page.tsx`
- `app/SiloUI/src/features/computers/components/computer-configuration-list.tsx`
- `app/SiloUI/src/features/computers/components/computer-list.tsx`
- `docs/SiloUI-FAILURE-REPORTING.md`
- `app/SiloUI/src/features/application/pages/computer-detail-page.tsx`
- `app/SiloUI/src/features/application/components/lifecycle-control.tsx`
- `app/SiloUI/src/features/computers/model/computer-capabilities.ts`
- `app/SiloUI/src/features/computers/components/computer-editor.tsx`
- `app/SiloUI/src/features/computers/model/use-computer-editing.ts`

Calls: `exports.*`.
