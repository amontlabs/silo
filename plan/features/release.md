## Proposed, not confirmed by the owner

### For the owner to settle

- Who publishes a release: an agent end to end, a person who confirms with a second factor, or two approvals after a waiting period?
  - Recommendation: A person with a second factor, because one publisher credential reaches every user; with one maintainer there is no second approver.
- Does a release wait for a person to run the candidate by hand: you, someone who did not build Silo, both, or nobody?
  - Recommendation: You on every release, and an outsider only before the first public one, since finding one each time would hold fixes back.

### For whoever builds it

- The release routine
  - Detail: Cutting a release is one routine an agent can run up to the publish step, so a merged fix reaches users quickly.
  - Rule: Security fixes have a release path of their own, which assumes an exploit exists the day a flaw is public.
  - Rule: One release script updates every place a version appears (tags, GitHub releases, package registries, the docs changelog).
  - Test: A release check fails when the published version numbers disagree.
  - Detail: The changelog is generated from the plan's feature entries and the changesets, then checked against the release's diff for anything left out.
  - Rule: Each changelog entry is written for a user to read.
  - Detail: Each published artifact carries signed provenance that names the commit and the build that produced it.
  - Risk: Notarization can stay pending for many hours (13 once), so the release routine waits for it and the announcement comes after.
  - Risk: Apple developer enrolment, and a DUNS number if a company publishes, must be complete well before the first release, so check it when this feature starts.
  - Detail: After each release, an agent reads the release log and hunts for bugs in what changed.
- Before a release is announced
  - Test: The published build is installed from the public channel on a clean machine, and the documented install, the quickstart and the first command pass before any announcement.
  - Property: The build that ships is the build that was tested, with the same hash.
  - Test: The release pipeline (build, sign, notarize, publish) is rehearsed end to end, and every step passes before a release is announced.
  - Rule: A rewrite of a public interface is not published until the quickstart has run on it.
  - Test: A live check runs the documented install and the quickstart in a fresh Silo computer, and both pass.
  - Test: Every default Silo ships (recipe, preset, agent brief) is run end to end before a release, and each one passes.
  - Test: The release tooling lists the exact contents of every published artifact and fails on anything unexpected, such as source maps, private build configuration or signing material.
  - Test: Known downstream projects run their own tests against a release candidate, and a failure blocks the release.
- What ships
  - Property: The installer has no undeclared dependency and nothing to install by hand, and the app works without a separately installed command.
  - Detail: The command line and the Linux engine ship as a plain binary and as a Nix package, never only through an npm wrapper.
  - Risk: Security-minded users criticise `curl | bash`, so a release offers a signed download and an install script people can read.
  - Rule: No package is installed for a feature the user has not opened.
  - Detail: Experimental tools ship apart from the `silo` command, so trying one needs no local source tree.
  - Property: The app, the engine and each computer image stay within a size budget, measured at every release.
  - Detail: One channel or feed carries releases and nothing else.
- Other
  - Test: On a clean machine, one command builds Silo from source, and the release check proves it every time.
  - Test: Before each release, a fixed set of everyday tasks (fork, change, diff, merge) runs end to end, and one failure holds the release.

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
