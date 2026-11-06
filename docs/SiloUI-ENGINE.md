# Silo engine extraction

The Cargo workspace is rooted at `app/SiloUI/Cargo.toml`, with its lockfile at
`app/SiloUI/Cargo.lock`. The Tauri application remains the `silo-ui` package
(`app/SiloUI/src-tauri`); the Tauri-free library is the workspace member
`app/SiloUI/src-tauri/engine` (`silo-engine`). The other members are `silo-api`
(`app/SiloUI/api`), `silo-client` (`app/SiloUI/client`), `silo-host`
(`app/SiloUI/host`) and `silod` (`app/SiloUI/src-tauri/silod`), so `silod`
depends on the engine without depending on Tauri.

`silo-engine` owns channel identity and defines `EngineContext`, `EnginePaths`,
`EngineIdentity`, `Event`, `Audience`, `MainThread` and `ProcessControl`. The Silo
UI's `TauriEngineContext` supplies those capabilities. Backend module groups
move into the library incrementally; commands remain in `silo-ui` as adapters
until their group is extracted.

## Current extraction state

The build channel, runtime operation gate (including its process-wide
`OPERATIONS` instance), and image-cache repair logic now live in `silo-engine`.
`silo-ui` re-exports the gate and image-cache functions for backend modules that
have not moved yet. Every workspace member is a Cargo default member, so
`cargo test --locked` from `app/SiloUI` covers the engine tests.

## macOS VM window host seam

The macOS VM host must not depend on a Tauri webview. When the VZ display code is
extracted, keep VM creation and `VZVirtualMachineView` attachment in the engine,
and expose the attach operation through a host seam that accepts the native
`NSWindow` supplied by the process hosting the engine. The operation must use
`EngineContext::main_thread()` for AppKit work. `silod` can then supply its own
window while Fjord and Silo UI hosts retain their own window policy. The current
window creation remains in `silo-ui` until that extraction step.

## Silo.app is a client of `silod`

One engine runs per Mac and channel (95 A-1). Silo.app does not construct one: it keeps a
connection to `~/.silo/engine.sock` (`~/.silo-dev` for Silo Dev) and every Tauri command that
used to call the engine in-process forwards to the canonical `silod` method over that socket.

- **Crates.** `app/SiloUI/client` (`silo-client`) is the thin native client: framing, the
  handshake, request correlation by id and the session event feed. It depends on `silo-api`
  only. `silo-ui` uses it from `src-tauri/src/engine_client/`.
- **Launch.** `engine_client::install` runs in the Tauri setup. The launcher uses whatever
  engine already serves the socket, whichever app supplied it. With none, it registers the
  LaunchAgent that `Silo.app/Contents/Library/LaunchAgents/<identifier>.engine.plist`
  describes (`SMAppService`, from the calling bundle) and waits for the socket. If the agent
  needs approval it opens Login Items and says so. The connection is kept for the life of the
  app: it reconnects with backoff, resubscribes, and reconnects before the engine's per-connection
  request-id limit.
- **Commands.** Command names, argument names and result shapes are unchanged, so the React
  UI is unchanged. Each command is a small inner `async fn` over `&EngineClient` plus a thin
  `#[tauri::command]` wrapper, and has a forwarding test against `engine_client::testing::FakeEngine`,
  an in-process fake socket server. A canonical job is awaited (`EngineClient::run_job`), then
  acknowledged. Losing the connection while waiting never turns into a failed job. A method the
  engine does not implement, or a request for another device, comes back as the engine's typed
  `unsupported_remote_operation` error and reaches the UI as the unavailable state; nothing falls
  back to in-process work.
- **Events.** Every frame is relayed as `silo://engine-event`. `engine_client/relay.rs` also
  produces the established `silo://` events the UI listens to.
- **Windows.** Dialogs, windows, the tray, menus, the Linux desktop viewer shells (a guest webview
  that the engine's `display.viewer.*` feeds) and launching the chosen terminal and editor stay in
  Tauri. The macOS VM window belongs to `silod` (`display.show`).
- **Quit** (95 A-2). With computers running the user chooses Cancel, Keep running, or Stop and quit.
  Only Stop and quit asks the engine to stop them (`engine.shutdown`). Quitting, updating or
  crashing the app never stops computers.
- **Boundary.** `src-tauri/src/engine_boundary_tests.rs` fails if the crate starts constructing
  the engine, calls engine operations in-process, or depends on the engine's runtime crates.
  `silo-ui` does not link `silo-engine`, directly or transitively. The stateless host helpers
  (`applications`, `notifications`, `system_integrations`, `system_shutdown`, `terminal`, the
  channel and the installer lock) live in the `silo-host` crate (`app/SiloUI/host`), which both
  `silo-ui` and `silo-engine` depend on.
- **Packaging.** `scripts/embed_engine.py` embeds `Silo Engine.app` (the signed `silod`, its
  manifest and plist) under `Contents/Resources/SiloEngineDistribution/` and copies the agent plist
  into `Contents/Library/LaunchAgents/`. `silod` resolves its tools from its own bundle, so the
  engine also carries copies of the app's signed `msb`, `libkrunfw.5.dylib`, Git tools,
  `silo-macos-gateway` and `silo-network-bootstrap` (`Contents/MacOS`, `Contents/Frameworks`) and
  the `microsandbox/` and `git-support/` resources. `scripts/build_desktop.py` (both
  `desktop:build` and `desktop:build:debug`) and `scripts/package-macos-release.py` sign the
  tools, embed the engine, then re-seal the app. Fjord's embedded engine (from the engine release
  tarball, which holds only `silod`) still lacks these tools.
  It also writes `SiloEngineCodeRequirement`, the signer requirement `silod` checks at run time, into the
  engine's `Info.plist` before signing ([engine releases](SiloUI-RELEASES.md#engine-releases)).

## Signing the dev silod with a stable identity

`silod` is sealed ad hoc (`codesign --sign -`) by default. An ad hoc signature is different
for every build, so a Keychain item whose access list names the previous `silod` build refuses
the next one. The engine never waits for the "wants to use your confidential information"
dialog (it forbids Keychain UI for the whole process): the call fails and the client gets the
`credential_access_denied` error ("Allow Silo's engine to use its saved sign-in"). Unattended
work (launch migration, timers, startup) skips and retries later.

To make approvals survive rebuilds, sign the dev engine with a stable identity.

1. Create a self-signed code-signing certificate once: Keychain Access, Certificate Assistant,
   Create a Certificate, name `Silo Dev Signing`, identity type Self Signed Root, certificate
   type Code Signing. (Or use an Apple Development certificate.)
2. Check it: `security find-identity -v -p codesigning`.
3. Build with the identity in the environment:

   ```sh
   export SILO_ENGINE_SIGNING_IDENTITY="Silo Dev Signing"
   npm --prefix app/SiloUI run desktop:build:debug
   ```

   `scripts/embed_engine.py` passes it to `codesign` for the nested `Silo Engine.app`
   (hardened runtime and the virtualization entitlement are unchanged). Unset, it stays `-`.
4. The first time the engine hits an existing item, approve it once in Silo (or in Keychain
   Access, Get Info, Access Control, add `Silo Engine`), and choose Always Allow. The next
   build with the same identity matches the same designated requirement and is not asked again.
   (Correction from the live run: without a Team ID each item's partition list names the
   creating build's cdhash, so every rebuild is refused again. See the user-started repair
   below.)

**Keychain access repair.** `silod` never lets the Keychain prompt, so after an engine update
the items an earlier build made fail with `credential_access_denied`. `engine.credentials.status`
lists the engine's items (GitHub sign-in, token ledger, personal token, secrets vault, one proxy
CA per macOS computer) as accessible, denied or failed. `engine.credentials.repair` is the only
place prompts are allowed: a client calls it only from a user click; it allows Keychain UI for
that one call on its own thread, touches each denied item so macOS shows its dialog (choose
Always Allow), and forbids prompts again before it returns, even on an error or a panic. The
switch is process-wide, so unattended Keychain calls that overlap the call could also prompt;
one repair runs at a time.

The cargo-built `silod` used by tests and the hand-assembled bundles of live runs must be
sealed the same way: `codesign --force --sign "$SILO_ENGINE_SIGNING_IDENTITY" --options runtime
--entitlements app/SiloUI/src-tauri/silod/macos/Entitlements.plist <Silo Engine.app>`.

The engine bundle's `Info.plist` has no `LSUIElement`: `silod` sets its own activation policy
(accessory, regular while a VM window is open), and AppKit refuses that call for a bundle that
is already an agent app. If AppKit still refuses the accessory policy, `silod` logs it and runs
on.

## `silod` on a Linux host

`silod` also builds and runs as a headless daemon on Linux (x86_64 and aarch64), where it runs
Linux computers over KVM. The Silo app stays Mac-only; a Linux host is managed from another
device's engine through Connections. The macOS code paths are unchanged: everything below is
behind `cfg(target_os = "linux")`.

- **Build.** `cargo build -p silod` (from `app/SiloUI`, toolchain 1.94.0). It needs
  `pkg-config` and `libdbus-1-dev` (for keyring's optional Secret Service backend) but no
  GTK: the engine depends on `silo-host` without its `desktop` feature, which holds the GIO
  and GTK integrations the app uses (application discovery, autostart, notifications). Without
  it, application discovery lists nothing and system integrations read as unavailable. CI
  checks that `cargo tree -p silod -e normal` lists no GTK crate.
- **Layout.** A Linux engine is one directory: `<dir>/silod`, its bundled tools beside it
  (`msb`, Git), and `<dir>/resources/` with `engine-manifest.json` and the runtime resources
  (`microsandbox/manifest.json`, `microsandbox/<target>/lib/libkrunfw.so.5.6.1`).
  `paths::default_resource_dir` resolves `<dir>/resources` from the running executable, so a
  link such as `~/.local/bin/silo-remote-dev` finds the same directory. The release archive
  that produces this layout is phase 3 of the [remote hosts plan](hosts/PLAN.md).
- **Identity.** The channel comes from `resources/engine-manifest.json`, validated as strictly
  as a bundle's (regular file under 64 KiB, known channel, semver version, non-empty build,
  protocol range) except for the bundle identifier, which a Linux engine has none of. There is
  no code-signature check on Linux: nothing seals the directory yet. Signed Linux archives
  (for example Sigstore) are roadmap milestone 11.
- **Account and directory checks.** Before it reads anything from its directory, every entry
  point (`--agent`, `--remote-bridge`, `--remote-guest`) refuses to run as root or `nobody`
  (use a dedicated account in the `kvm` group) and checks the engine directory
  (`silod::install_trust`): every directory from `/` down to it, `silod`, every entry beside it
  and everything under `resources/` must belong to this account or root and be writable by
  nobody else (a sticky `/tmp` is refused too), and a link must stay inside the directory. The
  refusal names the path. Before its first `msb` launch, the agent checks
  `resources/microsandbox/manifest.json` against the runtime inputs compiled into the build and
  the SHA-256 of `msb` and `libkrunfw` against it
  (`silo_engine::dependencies::verify_bundled_runtime`); an engine without the runtime starts
  and reports that computers cannot start.
- **Paths.** Data and configuration: `$XDG_DATA_HOME/<identifier>` (default
  `~/.local/share/org.silo.dev` for Dev). Cache: `$XDG_CACHE_HOME/<identifier>`. Logs:
  `$XDG_STATE_HOME/<identifier>/logs`. A relative or empty XDG value is ignored, as the
  specification says. The private state directory and the engine socket stay where every
  client already looks on both platforms, `~/.silo` and `~/.silo-dev` (`engine.sock`,
  `engine.lock`, the owner record, Connections state), so `silo-host`, `silo-client` and the
  CLI need no Linux-specific socket path and the two channels stay apart.
- **Peer check.** The socket accepts only clients of the engine's own account. On Linux the
  check reads `SO_PEERCRED`, the effective UID the client had when it connected, which is what
  `getpeereid` returns on macOS.
- **Credentials.** The engine's `keyring` entries (GitHub sign-in, token ledger, personal
  token, secret values) go to a file store, `silo_engine::credential_file_store`, under
  `<data>/credentials/` (see [Secrets](SiloUI-SECRETS.md#linux-hosts)).
  `SILO_CREDENTIAL_STORE=secret-service` uses the desktop Secret Service instead; any other
  value except `file` stops startup. The first start records the store in
  `<data>/credential-store`, and a later start with a different value stops with a message
  naming that record.
- **Running it.** `silod --agent` (or no argument) serves until it is stopped. The process main
  thread runs the queue that `MainThread` work goes to, in place of the AppKit loop. SIGTERM
  and SIGINT both start the engine's bounded session-end shutdown, as SIGTERM and logout do
  on macOS; a second one of either exits at once with status 1. `msb` and guest commands run in
  their own process group, so a terminal's Ctrl-C reaches `silod` alone. Exit code 75 still
  means "restart me" for a supervisor. `--remote-bridge` and
  `--remote-guest` work through the `~/.local/bin` link. `silod --service` reports that it is
  not available yet: the systemd user unit is phase 3. Until then, start it by hand or from
  your own unit, for example a Dev engine run by a dedicated account (in the `kvm` group to
  start computers), installed where only that account can change it:

  ```sh
  mkdir -p ~/silo-engine/resources
  cp target/debug/silod ~/silo-engine/
  cp src-tauri/silod/macos/resources/engine-manifest.development.json \
    ~/silo-engine/resources/engine-manifest.json
  chmod -R go-w ~/silo-engine
  ~/silo-engine/silod --agent &
  target/debug/silo list
  ```

  A root-owned `/opt/silo-engine` works the same way. An engine under `/tmp`, or in a directory
  another account can write to, does not start. Group write counts even for a group only you are
  in, so a directory made under a umask of `002` needs the `chmod` above, and Debian's group-writable
  `/usr/local` is refused.

  This proves the daemon serves; creating or starting a computer also needs `/dev/kvm`, the
  runtime resources above and the guest image.
