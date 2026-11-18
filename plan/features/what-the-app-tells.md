## Proposed

- What the app shows about forks, tasks and diffs comes from the engine, never from what an agent reports. It holds when an agent was told nothing, or lies.
- The app shows something as done, passed or freed only once the engine confirmed it. When it shows an estimate, the real figure stays within it.
- The app shows a short summary of running work first. The fork tree, the rules and the events are one step away and never hidden.
- A rule can name a preferred choice and a fallback. Silo never falls back on its own to anything the rule did not name.

## Open

- Does the app interrupt the user only when an approval waits, a rule is broken or something runs where it was not expected? Finished and failed work would then show in the menu bar only.
- Does a summary Silo shows, such as a task's result, have a short fixed shape, or show the agent's prose within a limit? A merge request's note is prose by design.
