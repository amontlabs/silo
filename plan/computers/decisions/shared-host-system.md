# Where a team's shared computers run

## The choice

An agent's part, 24 November 2026. A team of six buys a Mac mini, leaves it on, and wants each of
them to run agents on it from their laptops. Guides written in 2026 describe exactly this: a Mac
mini with Tailscale and SSH, a macOS user for the agents, and "a single spare Mac can support
multiple team members if tasks are queued".

Apple's licence for macOS Tahoe 26, read on 24 November 2026, says in section 2I that you agree
"not to use the Apple Software, or any of its functionality, in connection with service bureau,
time-sharing, terminal sharing or other similar types of services, whether such services are being
provided within your own organization or to third parties". Section 2J adds that "only one user may
use the Apple Software at a time". Section 2B lets a company have "multiple individuals on a single
shared Mac Computer", which reads as one after another, and allows two macOS virtual machines per
Mac for development and testing, which Apple's Virtualization framework enforces.

The options:

- A Linux server for what several people share (recommended). A Mac that stays on runs the
  computers of one person, reached from that person's other devices (`main-on-several-devices`). A
  team's shared computers run on a Linux server the team owns (`linux-server-host`), where no such
  licence applies. It costs macOS computers for several people at once on one Mac.
- A shared Mac mini, several people at once. It costs a reading of Apple's licence that its own
  words, "within your own organization", argue against.
- A shared Mac mini, one person at a time. It costs every agent of the others while one person has
  it.

Recommended: a Linux server for what several people share. Whether Apple grants more under a
written agreement is to find out. The licence text says so is possible; an Apple engineer asked on
Apple's forum in 2026 could not say how to ask for one.
