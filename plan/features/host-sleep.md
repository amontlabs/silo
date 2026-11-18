## Proposed

Why: An agent left running on a laptop stops when the laptop sleeps.

- Test: Put the Mac to sleep in the middle of a run in a computer; after wake the run continues.
- Risk: macOS sleeps a closed laptop on battery whatever an app asks, so the menu bar says when work will pause.

## Open

- Does Silo keep the Mac awake by default while work runs, or only when you turn it on?
  - Suggested: By default while an agent or a task runs, with the reason shown in the menu bar.
- To find out: Whether a computer's clock and its open connections recover after the Mac sleeps.
