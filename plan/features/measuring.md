## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- The whole check and the engine's build-and-test loop each have a time budget, measured at every merge, and going over it is a fix item. (a check in `tools/ci/check.sh`)
  - Recommendation: yes. Agents make changes faster than a slow check can verify them.
- Before optimising, an agent checks whether the code matters, by removing it or adding a delay, and confirms any explanation with a second measurement. (`AGENTS.md`)
  - Recommendation: yes. Agents optimise what they guess is slow.
- A performance number counts only from repeated, interleaved runs on a Mac that is plugged in and cool. (`AGENTS.md`)
  - Recommendation: yes. One run before and one after on a laptop proves nothing.
- Every published number states the build mode it was measured in, such as ReleaseFast. (`AGENTS.md`)
  - Recommendation: yes. Zig's build modes differ enough to change any conclusion.
- The engine core has benchmarks from its first commit. (a check in `tools/ci/check.sh`)
  - Recommendation: talk it through. They catch a slowdown the day it lands, but numbers on a laptop are noisy.
