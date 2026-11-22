# When does a request wait for an extension's answer

## The choice

An agent's part, 22 November 2026. Main has two extensions. Disk watch reads /work every minute and
alerts you. Push check may hold main's pushes. When an agent pushes, the push waits for Push check's
answer. Nothing main does ever waits for Disk watch.

The options:

- Promise it (recommended). Silo promises that a request waits for an extension only when you
  installed one that may protect that computer, and Silo is tested for it. It costs nothing you pay.
  It rules out an extension that looks at each request before it goes without being able to stop it;
  such an extension reads the record after the fact instead.
- No promise of its own. The same holds today, as a result of how extensions are built, but nothing
  checks it. It costs the day a change makes your agents wait for an extension that only reads, and
  nobody notices.

Recommended: promise it. Installing an extension that only reads should never slow your agents, and
a promise with a check is how Silo keeps that true as it changes.
