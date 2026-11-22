# Every computer reached by name

## The choice

An agent's part, 22 November 2026. An agent makes a fork named try-bun. A moment later you type
`ssh try-bun` in your terminal, and your editor lists try-bun among its remote machines.

The options:

- Build it (recommended). Silo adds one line to your Mac's SSH settings, with your yes, and keeps an
  entry for each computer from then on. It costs a standing grant: that one yes lets your Mac into
  every computer, including those not made yet, and Silo shows it on each one.
- Leave it out. You add an entry for each computer to your SSH settings by hand. It costs a step for
  every fork, and agent apps that list your SSH machines do not see a new computer until you add it.

Recommended: build it. Forks come and go too often to be added by hand, and every tool you use to
work on another machine reads these settings.
