## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- Each screen is drawn as an image the owner approves before agents build it, and a screen that shows more than its image goes back. (`INTERFACE.md`)
  - Recommendation: talk it through. The app's look stays the owner's, at the cost of the owner's time on every screen.
- INTERFACE.md proves a visible change by driving the built app; instead, agents prove it through the command line and the event log, unless the change is in the app. (`INTERFACE.md`)
  - Recommendation: talk it through. Driving the app costs about ten times the context, but only the app shows what a user meets.
- How does an agent see the app it builds: by driving the Swift app, or through a web version of each screen that is ported once it settles? (`INTERFACE.md`)
  - Recommendation: talk it through. Driving the Swift app keeps one code base; a web step shows results sooner but builds each screen twice.
- The app can run against a fake engine client that plays recorded or simulated events, so screens are built and watched without a VM. (`INTERFACE.md`)
  - Recommendation: yes. Events are defined once (calls-and-events), so the fake client is cheap.
- In app code, state changes only where a user action or an engine event arrives, never in reaction to another state change. (`INTERFACE.md`)
  - Recommendation: yes. It matches the engine's events and removes a whole class of update loops.
- The owner judges visual polish, and agents are not sent back again and again for it. (`.atelier/rules/ORCHESTRATOR.md`)
  - Recommendation: yes. Polish is a matter of taste, where agent loops spend tokens without converging.
- Each new kind of app component starts from a named reference app, GitButler and Linear for the look, never from nothing. (`INTERFACE.md`)
  - Recommendation: talk it through. Copying beats inventing, but both are web apps, and a Swift app may need Swift references.
