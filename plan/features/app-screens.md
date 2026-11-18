## Proposed

- The app can run against a fake engine client that plays recorded or simulated events, so screens are built and watched without a virtual machine.
- In app code, state changes only where a user action or an engine event arrives, never in reaction to another state change.
- Visual polish is judged by a person. Agents are not sent back again and again for it.

## Open

- Is each screen drawn as an image and approved before agents build it? The look then stays in one person's hands, and every screen waits on them.
- How does an agent see the app it builds: by driving the Swift app, or through a web version of each screen, ported once it settles? Driving the Swift app keeps one code base; a web version shows results sooner but builds each screen twice.
- Is a change outside the app proven through the command line and the event log, instead of by driving the built app? Driving the app costs much more context, but only the app shows what a user meets.
- Which apps does a new kind of component start from? GitButler and Linear were proposed for the look; both are web apps, and a Swift app may need Swift references.
