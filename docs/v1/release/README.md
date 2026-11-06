# Silo v1 release preparation (slice 6c)

Nothing here was released, tagged, pushed or published. This is what the owner needs to run the release. Authoritative procedure: [`docs/SiloUI-RELEASES.md`](../../SiloUI-RELEASES.md). Rules: `AGENTS.md` (no tags or publication unless the owner asks). The owner approved the version bump and a release without Apple signing ("I don't have an apple developer id so we just do without signing"); both are done on branch `claude/silo-v1-release`.

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

`npm run release:version` was run: `silo-ui` is now **0.13.0** (from 0.12.0; 34 changesets consumed, 22 `minor`, 12 `patch`). It updated `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `Cargo.lock` and `CHANGELOG.md`, and wrote [`docs/releases/0.13.0.md`](../../releases/0.13.0.md), the update notes. An "Opening this version" paragraph at the top of those notes tells users how to open an app that is not notarized. "v1" is the product milestone, not the version number. A draft of the user-facing text is in [notes-draft.md](notes-draft.md).

The engine has its own version (`silod/Cargo.toml` and `engineVersion` in the engine manifests). Whether to cut an `engine-v<version>` release is the owner's call; this slice did not change it.

## Release without Apple signing

With none of the seven Apple secrets set in `release-signing`, a tag push or `draft` dispatch of **Build Silo release** now signs the app and DMG ad hoc and skips notarization; the run shows a warning. A partial set of secrets fails the run. Adding all seven later brings back Developer ID signing and notarization without a workflow change. The updater key `TAURI_SIGNING_PRIVATE_KEY` (and its password) is still required in `release-signing`. Details: [Releases without Developer ID signing](../../SiloUI-RELEASES.md#releases-without-developer-id-signing).

What someone installing it sees: the first open of `Silo.app` says macOS cannot verify the app, with no Open button. They open System Settings → Privacy & Security and click **Open Anyway**, or run `xattr -dr com.apple.quarantine /Applications/Silo.app` after copying it to Applications. Expected, not yet checked on a real update: in-app updates open without that dialog, and macOS may ask again for Keychain access after an update because the ad-hoc signature changes.

### Local unsigned build of 0.13.0 (2026-11-06)

Built on this Mac from commit `a1648949` (production identity; built only, not launched): `CARGO_TARGET_DIR=app/SiloUI/target/release-0.13.0 npm run desktop:build`, then `SILO_RELEASE_SIGNING=adhoc TAURI_SIGNING_PRIVATE_KEY=<throwaway test key> python3 scripts/package-macos-release.py <Silo.app> target/release-0.13.0/assets --engine <silod> --cli <silo>`. Output in `app/SiloUI/target/release-0.13.0/assets/` (ignored build output, not committed):

| File | Bytes | SHA-256 |
| --- | --- | --- |
| `Silo-macos-arm64.dmg` | 135996171 | `e816f873770df9bf81343562d356396906537db9cbac8e52ef473b3b0cd40f8a` |
| `Silo-macos-arm64.app.tar.gz` | 124033286 | `6288bde3df3ed35bae9a74f55c2ff1ce85390ea472a446ace1accedc79916baa` |
| `Silo-macos-arm64.app.tar.gz.sig` | 416 | `3633b4f9e20cc916217063b825c5ea06cbe131a72ae57c77165783c9bdfa1f6b` |

Checks on `target/release-0.13.0/release/bundle/macos/Silo.app`: `CFBundleShortVersionString` 0.13.0; `codesign -dv` shows `Identifier=org.silo.preview`, `Signature=adhoc`, flags `adhoc,runtime`, no team; `Contents/MacOS/silo` is present (`org.silo.cli`, ad hoc, hardened runtime; `silo --version` prints `silo 0.1.0`, the CLI crate's own version); `codesign --verify --deep --strict` passes; `spctl --assess --type execute` rejects it, as expected without notarization. The `.sig` is from a throwaway key, so these files are for inspection only: the release assets come from the workflow, which signs updates with the production updater key. Not installed or launched.

Tests: `python3 -m unittest discover -s scripts -p 'test_*release*.py'` exit 0, including the new `resolve-mode` tests in `test_release_notarization.py`.

## Owner's remaining steps, in order

1. **Apple signing: skipped for 0.13.0.** Leave the seven Apple secrets unset in `release-signing`; check that `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` are set there. For a later notarized release: Developer ID Application certificate as `.p12` and an App Store Connect API key, added as `APPLE_CERTIFICATE_P12_BASE64`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_DEVELOPER_ID_IDENTITY`, `APPLE_TEAM_ID`, `APPLE_API_KEY_P8`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER_ID` (checklist: `SiloUI-RELEASES.md#owner-checklist`).
2. **Push and merge.** `git push -u origin claude/silo-v1-release`, open a pull request to `main`, let CI pass, merge.
3. **Guest image.** If `guest-image/image-lock.json` changed since the last published image, publish the guest release first (**Publish Silo guest image**, environment `guest-image-publish` with required reviewers), because the app downloads the pinned assets directly.
4. **Review the version commit** (done on the branch): version files, `CHANGELOG.md`, `docs/releases/0.13.0.md`, removed changesets. Edit the wording before merging if needed.
5. **Draft.** From the clean, up-to-date `main` checkout in `app/SiloUI`: `npm run release:draft` pushes the `v0.13.0` tag, which starts **Build Silo release**; approve `release-signing`. The macOS job warns that it signs ad hoc. Check the draft on a downloaded copy: `codesign -dv Silo.app` shows `Signature=adhoc`; `spctl --assess` rejects it, as expected without notarization.
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
