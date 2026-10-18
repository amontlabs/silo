# Detached desktop viewer implementation plan

Status: implemented, 2026-09-28. Silo contains the separate guest
session/stream lifecycle, recovery and stopped-update actions, Tauri proxy fixes,
the Selkies first-frame fix, the LCU setup/status helper, and frontend
status/actions. Final verification passed on a disposable scratch computer, including
the ARM64 first-frame check, LCU doctor and x86 native input; the computer was stopped
afterwards. The [probe evidence](research/desktop-viewer-probe-2026-09-27.md)
records exact results and remaining limits.
This plan
supersedes implementation suggestions in the [viewer direction](SiloUI-DESKTOP-VIEWER-DIRECTION.md)
where they conflict. The earlier [optional-desktop plan](archive/SiloUI-DESKTOP-IMPLEMENTATION-PLAN.md)
records the preceding KasmVNC implementation, not this migration.

The evidence table and delivery steps below retain the September design
snapshot, including the former Kasm/Luda recipe. For current built-in desktop
and agent behavior, follow [Linux desktop](SiloUI-DESKTOP.md) and the later
[computer use plan](SiloUI-COMPUTER-USE-PLAN.md).

## Outcome and boundaries

Keep Tauri. Give each computer one persistent Xfce/X11 desktop, attach Selkies to it,
and present the same viewer for local and remote computers. LCU runs inside the guest
and acts on that desktop directly. Closing, resizing or reconnecting a viewer
must not change the desktop's geometry, apps or lifetime.

Human and agent input share the desktop. Concurrent input is the human's
responsibility. Do not implement input ownership, leases, arbitration,
takeover handshakes, agent pause/resume, cancellation or permission prompts
between human and agent actions. Keep direct human input as it works today;
no new interaction-mode toggle or observer credential system is needed.
Passive viewing means that connecting sends no unsolicited desktop mutations,
not that the human has lost permission to type or click.

This work preserves the existing optional-desktop installation, working account,
automatic/manual startup, local/remote ownership and explicit computer shutdown
semantics. No Electron runtime, new VM engine, desktop environment replacement,
public relay service or general remote-desktop framework is required.

## Evidence that determines the work

| Finding | Evidence | Implementation consequence |
| --- | --- | --- |
| The existing UI already embeds an isolated Tauri child webview and uses an authenticated loopback gateway | [desktop_viewer.rs](../app/SiloUI/src-tauri/engine/src/desktop_viewer.rs), [desktop_proxy.rs](../app/SiloUI/src-tauri/engine/src/desktop_proxy.rs), [capabilities](../app/SiloUI/src-tauri/capabilities/desktop-viewer.json) | Reuse these boundaries; qualify a different client inside them |
| Current session supervision launches `vncserver :1 -fg -autokill`; readiness uses its streaming port | [desktop-service.py](../app/SiloUI/src-tauri/guest/desktop-service.py) | Separate display/session health from stream health before claiming streamer independence |
| Current guest recipe pins KasmVNC 1.5.0, sets 1440×900, disables remote resize and installs Luda | [setup-desktop.sh](../app/SiloUI/src-tauri/guest/setup-desktop.sh) | Preserve geometry and working-account behavior; treat streamer and agent-tool migrations separately |
| Selkies 2.0.0 supplies native Ubuntu packages for both guest architectures and serves its web client/media through one TCP port | [release](https://github.com/selkies-project/selkies/releases/tag/2.0.0) | Pin one artifact per architecture and start with its WebSocket mode |
| Selkies can attach to an existing X11/audio session; its session launcher can instead create and own them | [native guide](https://github.com/selkies-project/selkies/blob/2.0.0/docs/native.md) | Run the attachment service, not `selkies-session`, as the replaceable streamer |
| Tauri uses WKWebView on macOS and WebKitGTK on Linux | [Tauri webviews](https://v2.tauri.app/reference/webview-versions/) | Test embedded engines on supported OS versions; Chromium success does not qualify Tauri |
| LCU 0.4.0 changed packaging and maintained harness adapters | [release](https://github.com/0xpolarzero/lcu/releases/tag/v0.4.0), [installation](https://github.com/0xpolarzero/lcu/blob/v0.4.0/docs/INSTALLATION.md) | Do not reuse the old LCU 0.2.1 or Luda installer assumptions |

Selkies is a selected candidate, not a demonstrated performance winner. The
first delivery gate must resolve WebKit decoding and independent-session
behavior before production migration. A JPEG fallback establishing visible
pixels alone is insufficient evidence of a good video/text experience.

The coordinated source/code reviews are retained in the
[viewer evidence](research/desktop-viewer-implementation-evidence-2026-09-27.md),
[guest lifecycle evidence](research/desktop-lifecycle-implementation-evidence-2026-09-27.md)
and [Selkies compatibility evidence](research/selkies-tauri-implementation-evidence-2026-09-27.md).
This plan resolves their recommendations into one scope; research alternatives
are not additional deliverables.

## Architecture to implement

```text
Computer: existing silo account and persistent filesystem
  silo-desktop lifecycle
    ├─ virtual X11 display, fixed initial geometry and X authorization
    ├─ D-Bus + Xfce + applications
    ├─ user audio service
    └─ separately restartable Selkies capture/streaming service

  Agent backend -> LCU -> same X11/accessibility session
                            (no dependency on Selkies or the viewer)

Guest Selkies -> existing owner connection / SSH forwarding
             -> authenticated loopback gateway
             -> restricted Tauri child webview

Human input -> Selkies -> same guest desktop
```

Start with the distribution's Xvfb and CPU encoding. The
[Ubuntu Xvfb manual](https://manpages.ubuntu.com/manpages/noble/man1/Xvfb.1.html)
establishes virtual display operation without display hardware. Do not assume
stock Xvfb has the GPU paths or dynamic monitor behavior of Selkies' patched
container image. Start with the current 1440×900 geometry. Qualify explicit
resolution changes against the exact server; if they require restarting the
desktop, label that consequence instead of implying a harmless resize.

The existing guest boot hook and supervisor remain the lifecycle entry point.
Extend the existing implementation narrowly to track separate children and
restart only Selkies on a stream failure. Do not add systemd assumptions or
another generic supervisor. Display/session failure is materially different:
restarting the display cannot preserve arbitrary X11 application connections.
Report that failure truthfully and retain the explicit desktop restart action.

## Delivery 1: prove the end-to-end path

Create one disposable, reproducible qualification harness under
`app/SiloUI/scripts/`, with generated artifacts under the existing ignored
`src-tauri/target/verification/` directory. It must operate on explicit scratch
computer identities, record all pinned versions, and leave unrelated computers/processes
untouched. Do not use a browser-only demonstration as acceptance.

1. Record the current KasmVNC control on the same applications, computer resources,
   device and network: cold/warm attach, input-to-visible-update latency, static
   text, scrolling/video, CPU, memory and transferred bytes.
2. Install the architecture-matched Selkies 2.0.0 Ubuntu 24.04 package, verifying
   its hash. Start display, Xfce/D-Bus and audio independently. Run Selkies as
   `silo`, pointed at those existing services, with automatic resize disabled.
3. Embed its bundled client in the actual Tauri child webview through the
   existing gateway. Probe the actual negotiated decoder/encoder, secure-context
   requirements, WebSocket paths, worker/WASM loading, audio and reconnects.
   Record the selected path; do not infer it from browser names or API presence.
4. Perform an independently checked edit/save through LCU in the guest, before
   a viewer opens, while it observes, after it closes and across streamer
   failure/restart. Keep an unsaved editor buffer open during the lifecycle test.
5. Repeat the compatibility slice on all three supported device builds and both
   guest architectures, including a connection through another physical owner.

**Exit:** real keyboard/pointer input, readable text, media playback and recovery
work in each supported webview; guest apps and agent actions survive streamer
and viewer loss; measured resources justify continuing. Record missing hardware
as untested, never as passing. If a supported engine cannot meet the contract,
fix a minimal reproducible upstream gap or reject Selkies before integrating it.
Do not make adding Electron the automatic fallback.

## Delivery 2: independent guest lifecycle and versioned installation

Change [desktop-service.py](../app/SiloUI/src-tauri/guest/desktop-service.py),
[setup-desktop.sh](../app/SiloUI/src-tauri/guest/setup-desktop.sh) and
[desktop.rs](../app/SiloUI/src-tauri/engine/src/desktop.rs). Add a small guest artifact
lock manifest following the existing pinned-runtime pattern.

- Keep the `silo` UID/home and existing `silo-desktop` entry point. Establish
  X authorization, one Xfce D-Bus session and one user audio endpoint. All desktop
  apps, LCU and Selkies target that same session. Do not disable X authorization.
- Track display/session and streamer process identities separately using the
  existing boot-ID/process-start checks. Give independent children distinct
  termination boundaries so killing the streamer cannot kill the desktop.
- Use bounded restart attempts and existing bounded logs. Exhausted stream
  retries produce a stream error while the desktop remains running. A desktop
  stop terminates all desktop-owned services; a stream reconnect terminates none
  of the desktop or agent processes.
- Keep the initial endpoint at guest port 6901, which `connection_local()`
  currently validates, and bind Selkies to guest loopback. The active viewer
  uses SSH forwarding directly to guest loopback for local and remote computers;
  `network::desktop_endpoint()` currently has no call sites. Verify the exact
  forwarding route in the spike instead of adding a public guest listener.
- Separate public `sessionState` and `streamState`, retaining compatible legacy
  fields during migration. Do not report the desktop stopped merely because
  port 6901 is closed. Probe the service actually selected by the recipe.
- Record recipe/backend versions, package hashes, configured geometry and
  installation completion. Status and viewer open are read/attach operations;
  neither downloads packages nor rewrites agent configuration.
- Preserve automatic/manual startup and explicit-stop-until-next-boot semantics.
  Preserve the existing distinction between closing a viewer and Silo Quit.
- Audit default connection hooks, keymap, DPI and clipboard behavior. Configure
  them so attaching without human input does not mutate guest settings. Fix unavoidable
  shared-component defects upstream, with a focused reproducer.

Write failing behavioral tests first for the new separation: streamer exit must
leave the recorded display and editor alive; viewer detach must not invoke a
desktop stop; failed stream readiness must not overwrite session state. Mocked
process tests need the live process-survival checks from Delivery 1.

## Delivery 3: transport and native viewer

Change [desktop_viewer.rs](../app/SiloUI/src-tauri/engine/src/desktop_viewer.rs),
[desktop_proxy.rs](../app/SiloUI/src-tauri/engine/src/desktop_proxy.rs),
[remote_access.rs](../app/SiloUI/src-tauri/engine/src/remote_access.rs) and the public
status projection in [desktop.rs](../app/SiloUI/src-tauri/engine/src/desktop.rs).

Keep the existing stable computer identity checks, remote-owner SSH bridge, loopback
cookie, origin restrictions and lack of privileged Tauri capabilities in guest
content. Keep local viewing independent of internet access. Reuse the shipped
Selkies web client; use its supported configuration hooks before writing custom
client code. Any required small client integration belongs in an upstreamable
patch, not a separate decoder or desktop protocol implementation.

The existing proxy injects Basic authentication and supports a bounded subset
of HTTP methods. Inspect the actual Selkies request/response contract before
extending it: main client, media/control WebSockets, authentication, redirects,
workers, WASM and any permitted clipboard operation. Preserve required media
headers. Test against the real upstream service, including rejected origins,
expired/replaced connections and wrong-computer access. Add only demonstrated missing
HTTP behavior; do not create a new proxy subsystem.

Keep the existing authorized human-input path and upstream Basic authentication
behind the native gateway. There is no new public sharing or viewer-role system
in this work. Never call the agent to pause, change permissions or surrender
input. Release only input held by the disconnecting viewer where the upstream
input implementation supports it; test stuck-modifier recovery without global
input resets. Do not replay buffered keyboard/pointer events after reconnect.

The backend connection receipt carries the selected recipe/protocol and the
private connection details. Public state excludes credentials. Legacy owners
and guests either use their existing Kasm path or return an explicit update
requirement; no silent interpretation of a Selkies endpoint as KasmVNC.

## Delivery 4: the minimal viewer experience

Change [linux-desktop-viewer.tsx](../app/SiloUI/src/desktop/linux-desktop-viewer.tsx),
[linux-desktop-state.ts](../app/SiloUI/src/desktop/linux-desktop-state.ts),
[linux-desktop-menu.tsx](../app/SiloUI/src/desktop/linux-desktop-menu.tsx) and their
existing fixture/tests. Build fixture states while Delivery 2 is in progress.

Keep the current dedicated viewer, computer identity, fullscreen and overflow menu.
Human input works directly without takeover language, agent status controls,
an interaction-mode toggle or a confirmation workflow. Preserve fit-to-window and
native-size viewing; window resizing/fullscreen only changes local presentation.
Use an explicit display-settings operation for guest geometry.

Distinguish these states with one appropriate action:

| State | Presentation/action |
| --- | --- |
| Computer or desktop stopped | Existing explicit start action |
| Desktop running, viewer connecting | Connecting indication, no desktop restart |
| Stream disconnected, desktop still running | Reconnect; retain a clearly marked stale frame or placeholder, never an apparently live frozen screen |
| Stream service failed | Restart stream; explain that applications remain running |
| Desktop session failed | Existing explicit desktop restart, with its application-closing consequence |
| Agent tools unavailable | Existing tools repair/readiness flow; viewing stays usable |

Keep guest/client advanced codec and protocol choices out of the ordinary UI.
Disable unsolicited clipboard synchronization. Provide intentional
copy/paste only where the qualified backend/client supports it; preserve the
existing file-transfer route instead of adding a second transfer product.
Do not enable microphone, camera, automatic monitor creation, gamepad or desktop
launch panels as side effects of replacing the streamer. Audio playback can use
the same view, subject to native webview activation requirements.

## Delivery 5: LCU in the guest

LCU compatibility is part of desktop qualification. Replacing the current Luda
installer is a separate, explicit change with its own failure state. It must not
be hidden inside viewer attachment or make a working desktop unusable.

Use the current LCU 0.4.0 contract, not the old prototype installer:

- On Linux, an official desktop app must already be installed in the guest.
  LCU copies its runtime into a private managed generation. It launches Node
  and the CUA REPL directly; its native path does not launch the Electron UI.
  That guest dependency is separate from the Tauri human viewer.
- Maintained adapters are Pi, Codex CLI and Claude Code. Do not claim the seven
  integrations in the current Luda recipe are automatically covered. Preserve
  unrelated and legacy harness configuration when adding supported LCU entries.
- Pin the LCU archive per guest architecture and record the actual app/runtime
  version selected. Establish the supported acquisition/distribution route for
  the prerequisite before promising a preinstalled image. Do not embed binaries
  from old LCU archives to bypass the current installation contract.

The Silo helper contract was checked against upstream tag `v0.4.0`: the Linux
installer accepts `--runtime-only`, `--user`, `--prefix` and `--existing-app`,
and checks for the selected official app before apt or installation writes
([installer source](https://github.com/0xpolarzero/lcu/blob/v0.4.0/scripts/install.py#L132-L185)).
The setup CLI accepts `--agent auto`, `--session direct`, `--yes`, `--user` and
`--prefix`; its registration output identifies each completed phase, and its
release metadata records `package_version`, `runtime` and `architecture`
([setup source](https://github.com/0xpolarzero/lcu/blob/v0.4.0/lcu/setup.py#L463-L522),
[setup options](https://github.com/0xpolarzero/lcu/blob/v0.4.0/lcu/setup.py#L613-L631),
[install metadata](https://github.com/0xpolarzero/lcu/blob/v0.4.0/scripts/install.py#L88-L117)).
The helper runs `doctor --non-interactive --require-ready`; both flags are
upstream-supported ([doctor source](https://github.com/0xpolarzero/lcu/blob/v0.4.0/lcu/doctor.py#L259-L265)).
Its `lcu-session` wrapper discovers one XFCE session for the selected UID and
passes its display, Xauthority and D-Bus environment to the command
([session source](https://github.com/0xpolarzero/lcu/blob/v0.4.0/lcu/session.py#L10-L42));
the helper’s explicit `direct` setup therefore uses that active session without
creating another desktop.

The helper and passive guest status now check the official `/usr/lib/chatgpt`
directory before trusting any prior LCU receipt. A focused regression with a
stale ready receipt confirms missing app reports `needs-runtime`; the action
returns that prerequisite through refreshed desktop status rather than claiming
setup success. The current [Luda integration](SiloUI-LUDA.md) remains separate.

Install under the guest desktop account. Reuse upstream Xfce session discovery
for SSH-backed agents, or pass the exact display/Xauthority/D-Bus environment in
direct mode. Use upstream setup and instructions rather than recreating the
agent tools. Verify `doctor`, a screenshot, a semantic action and an independent
saved-file oracle. Missing prerequisites produce a precise repair requirement;
they do not start a human viewer or install host automation tools.

When implementing this step, add the pinned LCU manifest/setup helper beside
the existing guest tools recipe, expose agent-tool identity/readiness without
mislabeling LCU as Luda, and update named registrations deliberately. Keep old
running tool processes and runtime generations until their sessions exit.
Changing the desktop viewer must not rewrite all harness settings.

## Delivery 6: safe migration, qualification and rollout

An active KasmVNC X server cannot be replaced by Xvfb while preserving arbitrary
open X11 application connections. Do not promise a live display migration.

1. New scratch computers receive recipe 2 after Delivery 1 passes. Existing computers retain
   their active Kasm session and receipt. Keep the two concrete implementations
   during migration; do not build a plugin abstraction for hypothetical backends.
2. Stage verified packages/configuration without replacing the running display.
   Activate the new recipe only at an explicit desktop update/restart or an
   authorized computer restart. Explain that desktop applications close. Opening or
   closing the viewer must never trigger migration.
3. Retain the previous working recipe, configuration and required packages until
   the new recipe is verified. Recovery switches the selected recipe back at a
   stopped-desktop boundary. It does not recover unsaved application state or
   make arbitrary apt transactions reversible.
4. Exercise interrupted download/install, wrong hashes, disk exhaustion, stale
   sockets, conflicting custom configuration, computer stop during install, owner
   disconnect and old/new owner-controller combinations. Preserve terminal/SSH
   access and a truthful retryable installation state.
5. Audit the actual Selkies package and codec dependency inventory, license
   notices, corresponding-source requirements and update/rollback mechanism.
   The [project license](https://github.com/selkies-project/selkies/blob/2.0.0/LICENSE)
   and [development guide](https://github.com/selkies-project/selkies/blob/2.0.0/docs/development.md)
   distinguish the project from its codec build variants; a project-level
   license label alone does not qualify the distributed artifact.
6. Complete platform evidence, bundled help, the desktop behavior docs and a
   minor changeset for the compatible feature. Follow the existing release
   process; this plan does not authorize publishing or changing VM engines.

## Verification matrix and measurement rules

| Device client | Required local guest | Required remote coverage |
| --- | --- | --- |
| Apple Silicon, macOS 14 minimum and current supported OS | ARM64 Ubuntu 24.04 | ARM64 and x86-64 guests on separate owners |
| Linux x86-64, Ubuntu 24.04-compatible | x86-64 Ubuntu 24.04, real KVM | ARM64 and x86-64 guests on separate owners |
| Linux ARM64, Ubuntu 24.04-compatible | ARM64 Ubuntu 24.04, real KVM | ARM64 and x86-64 guests on separate owners |

Record Linux display session type and exercise X11 and Wayland device sessions
where supported by the application. The guest remains X11. Record actual
WebKitGTK/WKWebView, codec, GPU/CPU path and package versions. Emulation and
cross-compilation are useful development checks, not substitutes for live
qualification on the release architectures.

Mandatory behavior checks: no-view LCU operation; connect/close/reconnect;
streamer crash; desktop-stop/computer-stop boundaries; geometry/DPI/keymap/clipboard
unchanged by observation; second observer; wrong-origin/wrong-computer rejection;
real keyboard layouts, modifiers, dead keys, Unicode/IME, scrolling, focus,
HiDPI/fullscreen; audio activation/recovery; remote loss and reconnect. Concurrent
human/agent input is permitted, so tests must not expect either to acquire a
lock or pause the other. Snapshot/restore tests cover only capabilities already
supported by the pinned VM engine.

Measure separately: guest session, guest streamer, guest LCU runtime, host Silo,
host webview helpers and total host footprint. Compare no viewer, one static
viewer, scrolling/video and two observers. Do not double-count shared resident
pages; use the appropriate OS memory accounting and report its definition.
Record cold/warm attach, median/p95 input-to-frame latency, frame drops, CPU,
bandwidth and text quality at equal resolution and comparable quality.

Freeze numeric resource/latency budgets after measuring the existing baseline
on the minimum target devices, before tuning the candidate. Store those budgets
with the reproducible benchmark. No absolute performance numbers in this plan
are presented as measured. Selection requires a documented quality/performance
benefit or removal of the coupled-lifecycle failure, with explicit accounting
for regressions. Do not call the result best-in-class from feature lists alone.

Zero-viewer capture suspension is unproven in the pinned documentation. Measure
it rather than adding viewer-count lifecycle coupling preemptively. Use upstream
stream start/stop behavior to release encoding work when available; any required
fix must leave Xvfb, audio, applications and LCU alive. Closing the native child
webview must also release its decoding resources.

Extend focused tests first:

```sh
python3 -m unittest discover -s app/SiloUI/scripts -p 'test_desktop_*.py'
npm --prefix app/SiloUI test -- src/desktop
npm --prefix app/SiloUI run typecheck
npm --prefix app/SiloUI run lint
cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml desktop
```

Follow with the relevant release-tooling checks, the guest tool setup tests for
Delivery 5 and the full required checks for the final change. Native commands
need the release guide's configuration; do not expose credentials in logs.
Build and inspect exact packaged artifacts and record fixture versus live
evidence. Documentation checks alone do not qualify this implementation.

## Sequence and completion

Delivery 1 gates production work. After it passes, guest lifecycle and fixture
UI can proceed in parallel. Transport integration follows the real guest
connection contract. LCU installation can proceed independently once its
prerequisites are resolved. Migration and final platform qualification depend
on those completed paths. Avoid calendar estimates until the compatibility
spike has measured the unresolved decoder and lifecycle work.

Done means the same user experience passes the supported matrix, applications
and LCU survive viewer/streamer loss, migration is recoverable at the documented
boundary, and the measured footprint is reported. No human/agent coordination
component is part of completion.

**First action:** build the disposable Tauri/Selkies compatibility harness and
run it against the existing KasmVNC baseline before changing production recipes.
