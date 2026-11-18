## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes becomes a line of `plan/decisions.json`.

- Silo is complete on one Mac, needing no hosted service, no server of the user's and no other user, and works with no server reachable.
  - Recommendation: yes. first-user is one person on a Mac, and a later Silo cloud then adds to the product without holding it up.
- Anything a hosted part of Silo does has a second path that works without it.
  - Recommendation: talk it through. It holds for a relay or a sync, but cloud-forks exists because the Mac has no room left.
- Checkpoints, logs and diffs stay on the device that made them, and copying them anywhere else is always an act the user starts.
  - Recommendation: yes. cloud-forks and backup-host then need the user to turn them on, which they should anyway.
- Silo sends no usage or crash data unless the user turns it on, and it lists plainly everything it sends off the device.
  - Recommendation: yes. selling-extras already plans the usage counters as opt-in.
- Everything Silo needs on the device ships in its own package, and it asks the user to install nothing else.
  - Recommendation: yes. vm-runtime already ships the VM program pinned; the rest follows the same way.
- When a piece Silo needs is missing or broken, only the feature that needs it stops and the rest keeps working.
  - Recommendation: yes. A failed GitHub sign-in should never stop the computers.
- No hosted part of Silo opens to other users before it is audited for abuse and runaway cost, and its sign-in and account recovery work.
  - Recommendation: yes. It binds only the day a Silo cloud exists, and costs nothing to agree now.
- An expired device link, sign-in or approval never puts data at risk; logging in again brings everything back as it was.
  - Recommendation: yes. An expired link to a server must never strand the computers on it.
