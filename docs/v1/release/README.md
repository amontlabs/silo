# Silo v1 release preparation (slice 6c)

Nothing here was released, tagged, pushed or published. This is what the owner needs to run the release. Authoritative procedure: [`docs/SiloUI-RELEASES.md`](../../SiloUI-RELEASES.md). Rules: `AGENTS.md` (no version bump, no consuming changesets, no tags or publication unless the owner asks).

## What a v1 release contains

| Part | What it is | Where |
| --- | --- | --- |
| App | The Silo macOS app (DMG and in-app update archive), Apple Silicon, macOS 15+ | `app/SiloUI`, workflow **Build Silo release** |
| `silod` | The engine, shipped inside the app and as its own release `engine-v<version>` | `app/SiloUI/src-tauri/silod`, workflow **Publish Silo engine** |
| CLI | The `silo` command and MCP server (`silo mcp`) | `app/SiloUI/cli` (crate `silo-cli`, binary `silo`) |
| Guest images | Linux guest image (arm64 and amd64) the app downloads on first use | `app/SiloUI/guest-image`, workflow **Publish Silo guest image** |
| `silo-action` | Composite GitHub Action: fork, run, review, open a pull request | `silo-action/`, to be published as `amontlabs/silo-action` |
| Reviewer service | The hosted reviewer behind `silo review` (Pro) | `reviewer-service/`, not deployed by any workflow |

The `silo` CLI is built with the app (`build_desktop.py` locally; `-p silo-cli` and `package-macos-release.py --cli` in the release workflow), signed like the other helpers and linked on PATH by the app (slice 6e); it is not a separate release asset. See [artifacts.md](artifacts.md).

## Version

Computed with the repo's own preview (`npm run release:status`, then `changeset status --output`; saved in [dry-run/](dry-run/)): `silo-ui` goes from **0.12.0 to 0.13.0**. There are 34 pending changesets (22 `minor`, 12 `patch`). The largest bump wins, and the rule is minor while below 1.0.0 (patch for fixes, never major; the scripts refuse 1.0.0 unless `--allow-stable`). "v1" is the product milestone, not the version number. The version is only fixed when the owner runs `npm run release:version`; that step also writes `docs/releases/0.13.0.md`, which becomes the update notes. A draft of the user-facing text is in [notes-draft.md](notes-draft.md).

The engine has its own version (`silod/Cargo.toml` and `engineVersion` in the engine manifests). Whether to cut an `engine-v<version>` release is the owner's call; this slice did not change it.

## Owner's remaining steps, in order

1. **Apple signing.** Developer ID Application certificate (only the Account Holder can create it), exported as `.p12`; App Store Connect API key (`.p8`, key ID, issuer ID). Add the seven secrets to the `release-signing` environment: `APPLE_CERTIFICATE_P12_BASE64`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_DEVELOPER_ID_IDENTITY`, `APPLE_TEAM_ID`, `APPLE_API_KEY_P8`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER_ID`. Checklist: `SiloUI-RELEASES.md#owner-checklist`. Keep a backup of the `.p12`, its password and the `.p8`.
2. **Dry-run the workflow in GitHub.** Dispatch **Build Silo release** with `dry_run` (no `draft`) and read the printed `[dry-run]` notarization steps. Not run here: it needs GitHub Actions.
3. **Guest image.** If `guest-image/image-lock.json` changed since the last published image, publish the guest release first (**Publish Silo guest image**, environment `guest-image-publish` with required reviewers), because the app downloads the pinned assets directly.
4. **Prepare the release** (from `app/SiloUI`): `npm run release:status`, `npm run release:version`. Review the diff (version files, `CHANGELOG.md`, `docs/releases/0.13.0.md`, removed changesets), edit the wording from the approved text, commit, merge to `main`.
5. **Notarized draft.** From the clean `main` checkout: `npm run release:draft` pushes the `v0.13.0` tag; approve `release-signing`. Check the draft on a downloaded copy: `spctl --assess --type execute -vv Silo.app` and `xcrun stapler validate Silo-macos-arm64.dmg`.
6. **Acceptance evidence** (`SiloUI-RELEASES.md#required-release-acceptance-evidence`): clean install from the DMG, a real update from the previous version, the failure cases listed there, and the computer-use live tests. No public-release claim without it.
7. **GitHub release.** `npm run release:publish`; approve `release-publish`. Publication is done only when **Publish verified Silo draft** succeeds.
8. **Engine release** (if the engine version was raised): run **Publish Silo engine** from `main`.
9. **Publish `silo-action`.** Create the `amontlabs/silo-action` repository from `silo-action/`; the `uses: amontlabs/silo-action@v1` example in its README needs a `v1` tag there.
10. **Deploy the reviewer service.** Follow `reviewer-service/README.md` ("Deploy (for the owner)"): pin the Python image digest, lock the requirements, build, set `ANTHROPIC_API_KEY` as a secret, hand each user a URL and token.
11. **Post the upstream drafts** in [`docs/v1/upstream/`](../upstream/): `msb-reclaim-capture-dirs`, `msb-rename-stopped`, `msb-retired-layer-cleanup`, `msb-secret-usage-events`, `msb-virtiofs-unlinked-checkpoint`.
12. **Approve public text**: the release notes, the credentials sentence, and the README, demo and numbers drafts (slice 6d).

Also open from the build log: Keychain repair for the Dev engine (the phase 1, 3 and 4 checks that needed it) and the leftover test computers to drop.

## Dry run

Outputs are in [dry-run/](dry-run/).

| Check | Result |
| --- | --- |
| `npm run release:status`, `changeset status --output` | pass; plans `silo-ui` 0.12.0 → 0.13.0 (minor); nothing consumed |
| `npm run test:release` | exit 0: 146 tests, 129 pass, 0 fail, 17 skipped |
| `python3 -m unittest discover -s scripts -p 'test_*release*.py'` (the set the draft job runs; includes `package-macos-release.py` in `dry-run` mode with notarization stubbed) | 174 tests, OK, 2 skipped |
| `python3 -m unittest discover -s scripts -p 'test_release_notarization.py'` (AMO-63) | 14 tests, OK |
| `reviewer-service` tests (`python3 -m pytest -q`) | 32 passed |
| `silo-action/test/run.sh` | **not run**: this session's shell guard refused to run it (it runs `sh` on a nested script). Run `sh silo-action/test/run.sh` |
| `SILO_RELEASE_SIGNING=dry-run python3 scripts/package-macos-release.py <Silo.app> …` on a real bundle | **not run**: needs a built `Silo.app` and `silod`, and there is no native build in this worktree (a native build needs the GitHub App configuration, which this task leaves alone). The unit tests above cover the step order and the dry-run stubs; the CI `dry_run` dispatch (step 2) covers the real bundle |
| `release:version`, `release:draft`, `release:publish` | deliberately not run (they consume changesets, tag or publish) |

Setup: `npm ci --ignore-scripts` in `app/SiloUI` (it printed npm audit findings; not looked into). The 17 skips in `test:release` are opt-in live tests. No secrets, signing data or `github-build.local.json` were read or printed.
