## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes becomes a line of `plan/decisions.json`.

- Everything the app shows about forks, tasks and diffs comes from the engine, and holds whatever each agent was told or did.
  - Recommendation: yes. An agent that was told nothing, or that lies, still shows up in the diff.
- The app interrupts the user only when an approval waits, a rule is broken or something runs where it was not expected.
  - Recommendation: talk it through. notifications also announces finished and failed work, which this would keep to the menu bar.
- The app shows a short summary of running work first; the fork tree, the rules and the events are one step away and never hidden.
  - Recommendation: yes. It fits pages-are-short without hiding the mechanism.
- What the app shows as done, passed or freed is what the engine confirmed, and any estimate it shows holds.
  - Recommendation: yes. The user acts on what the app says without checking it again.
- A rule may name a preferred choice and a fallback, and Silo never falls back on its own to anything the rule did not name.
  - Recommendation: yes. The containers decision already says it for the kind of computer; this extends it to every rule.
- A summary Silo shows, such as a task's result, has a short fixed shape and never shows the agent's prose as it is.
  - Recommendation: talk it through. A merge request's note is prose by design; the choice is whether it is shown bounded or replaced by fixed fields.
