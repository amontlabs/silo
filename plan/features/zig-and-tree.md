## Proposed

- `zig build` is the one entry point to build, test and check the engine and the Swift glue. `tools/ci/check.sh` calls it.
- One Zig version and one Swift version are pinned in one file, and the build refuses any other.
- A check rejects Zig forms older than the pinned version, such as the old ArrayList and JSON calls.
- Pinned, read-only copies of each dependency's source (Zig's standard library, MicroSandbox, Apple's headers) and of the reference code sit beside the tree, so agents read the pinned API instead of recalling an older one.

## Open

- Is each feature one folder holding its Zig core, its Swift side and its docs, or does the core stay one piece, with each feature listing its files? A folder per feature is easy to find, but it cuts across a core meant to be read whole.
- Is every dependency vendored in the tree, pinned at a tag, with nothing fetched at build or install time? A build then never breaks on a vanished repository, but copies of MicroSandbox and libkrun cost disk and upkeep.
