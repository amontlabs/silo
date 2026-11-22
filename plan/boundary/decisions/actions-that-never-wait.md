# Does stopping a computer or taking back its access wait for an ask

## For the owner

An agent's part, 22 November 2026. A rule you wrote stops a fork when it connects to a host that is
not on its list. At 03:00 the fork does, while it is halfway through a long build.

The options:

- Never wait (recommended). The stop happens at once. So does taking back a secret or closing a
  network, whoever triggers it. It costs the work in progress when a rule stops a computer at a bad
  moment, since a stop ends the programs running in it.
- A rule's stop asks you first. Taking back a secret and closing a network still happen at once, and
  a stop written in a rule waits for your yes. It costs the rule its use, since the fork keeps
  running until you answer.

Recommended: never wait. An action that only takes something away must not depend on you being
there. When losing the work in progress matters more, the rule can say pause where it said stop.

## Notes

What kind each of these actions is: `kinds-of-action` and `why-destructive` in `rules`.
