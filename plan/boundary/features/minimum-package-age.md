# A minimum age for packages

## For the owner

An agent's part, 22 November 2026. A rule on main denies packages younger than 7 days. An agent runs
`npm install`, which picks a release published two hours ago. The download is denied, and so is the
same file fetched with `curl`.

The options:

- Build it (recommended). A release that was poisoned and pulled within a week never reaches main.
  It costs the fresh releases you do want: a security fix published today is denied for 7 days
  unless you allow it. Silo also has to read the traffic to each registry, and whether it can is an
  open question. Each registry (npm, PyPI, crates.io) is separate work.
- Leave it out. You set the minimum age in each package manager, as pnpm and uv now allow. It costs
  the limit itself with a hostile agent, which fetches the package by hand.

Recommended: build it, once it is known that Silo can read registry traffic. Package managers added
this setting for a reason, and agents install more packages than people do.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

It assumes the boundary can read the traffic to package registries. That is the open question
`registry-traffic-read`.
