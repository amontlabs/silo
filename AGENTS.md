# Silo: development and verification

The application lives in `app/SiloUI/` and uses React/TypeScript with a Rust/Tauri backend. Run the commands below from the repository root unless a command says otherwise.

## Reuse established tools

Prefer maintained, well-recognized tools and supported upstream features over
custom infrastructure. Before adding a proxy, protocol, credential broker,
scheduler, storage mechanism, or similar subsystem, research existing solutions
and record the choice and concrete gaps in `docs/`. Evaluate the specific
component's threat model, maintenance, license, deployment fit, and supported
release; a familiar vendor name alone is not sufficient evidence. Keep Silo code
focused on product policy and integration. Fix reusable gaps upstream where
practical. A PoC implementation is not the default production implementation;
custom infrastructure needs evidence that suitable existing tools cannot meet
the requirement.

## Source and layout

- `app/SiloUI/src/`: UI, production data sources, deterministic fixtures, and frontend tests.
- `app/SiloUI/src-tauri/src/`: runtime management, connections to other devices, native integrations, and backend tests.
- `app/SiloUI/src-tauri/`: guest scripts, runtime inputs, capabilities, and Tauri packaging configuration.
- `app/SiloUI/scripts/`: runtime preparation and release tooling.
- `app/SiloUI/docs/`: bundled application help.
- `docs/README.md`: index of implementation documentation, research, and proposals.
- `docs/SiloUI-VOCABULARY.md`: the product terms (computer, device, Connections, workspace, sandboxed) that UI, code, saved data and documentation use.
- `docs/SiloUI-RELEASES.md`: authoritative build configuration and release procedure.

Preserve bundled MicroSandbox and Git tools, guest scripts, the Rust vendor patch, and the `silo-remote` SSH bridge. They are part of the current app. Generated `node_modules`, `dist`, Cargo `target`, binaries, and runtime resources are ignored build output; never commit them or private local configuration.

## Setup and commands

Use Node.js 24, Python 3.11 or newer, Rust, and the device's Tauri prerequisites. Runtime preparation (`npm --prefix app/SiloUI run runtime:prepare`, also run by `desktop`) requires Rust 1.94.0 for the pinned MicroSandbox build, Go 1.25, and network access on a cold cache. Supported packages target Apple Silicon macOS 14+ and Linux x86-64/ARM64 on Ubuntu 24.04-compatible systems; Linux VMs require KVM.

Native builds and Rust tests require GitHub App configuration. Follow `docs/SiloUI-RELEASES.md#local-setup`; do not print `github-build.local.json`, signing credentials, or verbose build output containing configuration. For native unit tests only, the release guide permits explicit synthetic GitHub configuration. Never distribute those test executables.

```sh
npm --prefix app/SiloUI ci
npm --prefix app/SiloUI run desktop
```

`desktop` prepares runtime resources and launches native development mode. `npm --prefix app/SiloUI run dev` starts the browser UI preview only.

Run checks appropriate to the change:

```sh
npm --prefix app/SiloUI run typecheck
npm --prefix app/SiloUI run lint
npm --prefix app/SiloUI test
cargo +1.94.0 fmt --manifest-path app/SiloUI/src-tauri/Cargo.toml --check
cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml --locked
npm --prefix app/SiloUI run test:release
```

Frontend tests use Vitest. Use test file arguments to focus a frontend run and a Cargo test filter to focus backend behavior. Rust tests live beside the code in `app/SiloUI/src-tauri/src/` and run with the default parallel thread count. Tests that reach process-wide state take the shared isolation guard; cache-lock tests use separate child processes to isolate file descriptor inheritance. See [native test support](docs/SiloUI-RUST-TEST-SUPPORT.md) for the remaining internally serialized group. `.github/workflows/ci.yml` runs these checks, and a relative-link check of the Markdown documentation, on every push to `main` and every pull request; `.github/workflows/linux-packaging.yml` builds and installs the Linux packages when packaging inputs change and nightly. Keep opt-in live tests separate from ordinary unit tests. Release-tooling changes also need the relevant script tests and workflow checks.

For a local macOS debug bundle:

```sh
npm --prefix app/SiloUI run desktop:build:debug
```

Output: `app/SiloUI/src-tauri/target/debug/bundle/macos/Silo Dev.app`.

For an optimized local macOS app without installer or updater signing:

```sh
npm --prefix app/SiloUI run desktop:build
```

Output: `app/SiloUI/src-tauri/target/release/bundle/macos/Silo.app`. On macOS, this command creates only a local app, applies the existing exact-engine VM signing policy, and verifies the final bundle. No distribution certificate or updater key is required. The previous explicit `--bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'` arguments remain supported. Linux builds retain the native Tauri packaging behavior. Follow the release guide for distributable packages; a local app build does not publish anything. Set an absolute `CARGO_TARGET_DIR` to rebuild separately from an app currently running from the usual output.

## Build channels

Only two builds exist and they never share state: production (`org.silo.preview`,
"Silo": `desktop:build`, releases) and development (`org.silo.dev`, "Silo Dev":
`npm run desktop`, `desktop:build:debug`, any `--debug` build). Every device-specific name
(home dir, Keychain services, remote bridge link, editor profile) comes from
`src-tauri/src/channel.rs`; never hard-code one. Production names must never change.
See `docs/SiloUI-BUILD-CHANNELS.md`.

## Running and debugging

- Use the Dev build (`Silo Dev`) for development and automation. Never drive, launch for testing, or modify the production app, its data, Keychain items, `~/.silo`, `~/.local/bin/silo-remote`, or VS Code `Silo` profile. Tests use temp HOMEs and fixtures. The owner runs `npm --prefix app/SiloUI run dev:import-production-settings`; agents must not run it against real data.
- Rebuild before inspecting a packaged change. Launch the exact bundle path with `open`, not an arbitrary installed copy with `open -a Silo`.
- Before a manual launch, inspect any existing instance and verify its executable path and ownership. Do not interrupt the user's running app or computers as routine test cleanup.
- Closing the window leaves the app running. Graceful Quit stops Silo-owned local computers; it does not stop computers on other devices. A shutdown failure leaves the app open. Account for these side effects before exercising Quit against real state.
- Use deterministic frontend fixtures for UI-only checks. Use semantic roles and identifiers instead of screen coordinates where available. Native UI automation requires an interactive session and the applicable OS permissions; do not reset permissions globally or dismiss unfamiliar security dialogs.
- Verify process identity before attaching a debugger. Prefer graceful cleanup of processes started for the test; never use `pkill`, `killall`, guessed PIDs, or routine `kill -9`.
- Keep generated evidence under an ignored directory such as `app/SiloUI/src-tauri/target/verification/`. Keep private system logs in a temporary local path and do not publish credentials, computer data, or unredacted logs. Preserve the exact failing output before rerunning.
- Report commands, results, the exact inspected bundle, and whether data was fixture or live. A frontend test proves UI behavior against its supplied data; a build proves compilation and packaging. Neither proves live computer health, two-device management, installed-app behavior, or release readiness.

## Delegated work

For larger tasks, work in your own git worktree, split the work into
independent slices and run them in parallel:

- Sonnet subagents implement.
- Haiku subagents take small, tightly bounded slices.
- Codex `gpt-6.1-sol` at high reasoning reviews each change, focused on that
  change. Fix and re-review until it answers LGTM.

## SiloUI release notes

For each user-visible SiloUI feature, fix, or behavior change, include a Markdown
changeset in `app/SiloUI/.changeset/` with `"silo-ui": patch|minor` front
matter and a concise user-facing summary. Agents may write the file directly.
Silo stays below 1.0.0 until the owner explicitly decides otherwise: use patch
for fixes and minor for features and incompatible changes. Never use major; the
release scripts refuse 1.0.0 or later without an explicit `--allow-stable` flag.
Internal-only changes need no changeset. Do not bump versions, consume
changesets, create release tags, or publish unless requested. Follow
`docs/SiloUI-RELEASES.md` for release preparation and verification.
