# May several computers write to one cache

## The choice

An agent's part, 22 November 2026. Main and a fork both build a Rust project, and its downloaded
packages take 3 GB. In the fork an agent tries a tool you do not trust yet.

The options:

- Each computer its own caches (recommended). The fork starts with main's 3 GB, since it shares
  main's disk, and what it adds stays in the fork. It costs downloads for a fresh computer, which
  fetches the 3 GB again, and the time that takes.
- One cache every computer writes. Nothing is downloaded twice. It costs a path outside every rule:
  the tool in the fork can replace a package or a build output in the cache, and main runs it at its
  next build. Attacks of this kind have been shown on GitHub's build caches.

Recommended: each computer its own caches. A fork already gets its parent's for free, and a cache
filled only by Silo from what registries sent can come later if fresh computers wait too long.

## Simulator runtimes and Xcode

An agent's note, 24 November 2026. Simulator runtimes (9.0 GB for iOS 27.3) and Xcode are signed by
Apple, so they fit the store outside every computer that only Silo fills: Silo can check each
against Apple's signature, as it would check a package against a registry's hash. Inside one
computer, several agents on the same repository each resolve Swift packages into their own build
folder, which this answer does not cover since it is about several computers. Details: `research/2026-11-24-mac-and-ios-developers/README.md`.
