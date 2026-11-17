## From the old code

The old code had it partly built, about 6,000 lines, and it needs reworking.

Lessons:
- Keep passwords out of command arguments: a failed subprocess prints its arguments.
- Stay below 1.0.0 until the owner decides; scripts refuse a stable version without a flag.
- Apple's library constraints already work with ad-hoc signing: a hardened msb loads only the exact bundled libkrunfw.
- The silo-ui build script printed the private GitHub build configuration into agent logs twice; a release script must never print configuration.
- Never publish on an incomplete signature matrix or overwrite version assets; a dry run must not create a tag.
- An embedded engine checks its signer requirement at run time, written into Info.plist before signing.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-RELEASES.md`
- `app/SiloUI/scripts/release.mjs`
- `app/SiloUI/scripts/package-macos-release.py`
- `app/SiloUI/scripts/macos_release_signing.py`
- `.github/workflows/release.yml`
- `docs/ROADMAP.md#L448`
- `docs/DIRECTION.md#L206`
- `docs/SiloUI-LIBRARY-CONSTRAINTS.md`
- `app/SiloUI/.changeset`
- `docs/v1/release/README.md`
- `app/SiloUI/CHANGELOG.md`
- `docs/SiloUI-RELEASES.md#L687`

Calls: `npm run release:version`, `npm run release:draft`.
