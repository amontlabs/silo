## Proposed, not confirmed by the owner

The owner says yes or no to each line. A yes is written in the file named after the line, or becomes the check it names.

- Is each feature one folder with its Zig core, its Swift side and its docs, or does its entry list its files, kept current by a check? (`AGENTS.md`)
  - Recommendation: talk it through. A folder per feature is easy to find, but it cuts across the core the owner reads as one piece.
- `zig build` is the one entry point for building, testing and checking the engine and the Swift glue, and `tools/ci/check.sh` calls it. (`AGENTS.md`)
  - Recommendation: yes. One obvious entry point keeps agents from inventing their own.
- Every dependency is vendored in the tree, pinned at a tag, and nothing is fetched at build or install time. (a check in `tools/ci/check.sh`)
  - Recommendation: talk it through. A build never breaks on a vanished repository, but copies of MicroSandbox and libkrun cost disk and upkeep.
- Pinned, read-only copies of the reference code and of each dependency's source (Zig's standard library, MicroSandbox, Apple's headers) sit beside the tree, listed in AGENTS.md. (`AGENTS.md`)
  - Recommendation: yes. Agents then read the pinned API instead of recalling an older Zig.
- One Zig version and one Swift version are pinned in one file, and the build refuses any other. (a check in `tools/ci/check.sh`)
  - Recommendation: yes. Zig changes its standard library between releases, so two versions in play means two dialects.
- A check rejects Zig forms older than the pinned version, such as the old ArrayList and JSON calls. (a check in `tools/ci/check.sh`)
  - Recommendation: yes. These are the mistakes agents make most in Zig, and a pattern search catches them.
- Does each engine subsystem get one reference codebase until a test shows more helps, or several from the start? (`AGENTS.md`)
  - Recommendation: talk it through. The evidence points both ways; starting with one costs less context and can grow.
- Each engine rule that no check can catch has a short good and bad example in the tree, next to the code it governs. (`AGENTS.md`)
  - Recommendation: talk it through. Examples teach better than a pointer to a doc, but each one must be kept true.
- AGENTS.md carries short Zig notes from the mistakes agents made: the pinned version's library changes and the built-ins they re-implement. (`AGENTS.md`)
  - Recommendation: no. The check and the examples teach the same forms without spending every agent's context.
