# Silo engine extraction

The Cargo workspace is rooted at `app/SiloUI/src-tauri/Cargo.toml`. The Tauri
application remains the `silo-ui` package; the Tauri-free library is the
workspace member `app/SiloUI/src-tauri/engine` (`silo-engine`). This keeps the
existing Tauri build and lockfile while allowing `silod` to depend on the engine
without depending on Tauri.

`silo-engine` owns channel identity and defines `EngineContext`, `EnginePaths`,
`EngineIdentity`, `Event`, `Audience`, `MainThread` and `ProcessControl`. The Silo
UI's `TauriEngineContext` supplies those capabilities. Backend module groups
move into the library incrementally; commands remain in `silo-ui` as adapters
until their group is extracted.

## Current extraction state

The build channel, runtime operation gate (including its process-wide
`OPERATIONS` instance), and image-cache repair logic now live in `silo-engine`.
`silo-ui` re-exports the gate and image-cache functions for backend modules that
have not moved yet. Both workspace packages are Cargo's default test members, so
`cargo test --locked` covers the engine tests.

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
  `silo-ui` still links `silo-engine` for five stateless host helpers (`applications`,
  `notifications`, `system_integrations`, `system_shutdown`, `terminal`); extracting them is open.
- **Packaging.** `scripts/embed_engine.py` embeds `Silo Engine.app` (the signed `silod`, its
  manifest and plist) under `Contents/Resources/SiloEngineDistribution/` and copies the agent plist
  into `Contents/Library/LaunchAgents/`; `scripts/build_desktop.py` runs it before signing the app.

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
