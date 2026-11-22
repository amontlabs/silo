# An engine the record and classifiers never hold up

## The choice

An agent's part, 22 November 2026. An agent runs a build in main that changes 40,000 files. At the
same moment you ask for a fork, and in another fork a classifier takes thirty seconds over one tool
call.

The options:

- Build it (recommended). The fork starts at once: what the build writes to the record travels on a
  path of its own and does not queue in front of your ask. In the other fork only that tool call
  waits, and if the classifier crashes or runs long, the engine carries on. It costs a second path
  to build and keep. When the build outruns the record, the record gets a gap that says so, and the
  engine is not slowed.
- Leave it out. Everything goes through one queue, which is simpler. It costs waiting: a large build
  or a slow classifier delays every ask to Silo, for every computer.

Recommended: build it. Large builds are what agents do all day, and Silo should not get slow because
of them.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

It may say again what `precaution-on-a-waiting-call` says about a request that waits for a classifier.
