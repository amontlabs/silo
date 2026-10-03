# One desktop, one viewer, every supported platform

Selected direction, 2026-09-27. This supersedes the split local-native/remote-
streamed recommendation in the [research synthesis](SiloUI-DESKTOP-EXPERIENCE-RESEARCH.md).
Silo implements this direction as of 2026-09-28; final scratch-computer
verification passed.
See the
[implementation plan](SiloUI-DETACHED-DESKTOP-IMPLEMENTATION-PLAN.md) and
[probe evidence](research/desktop-viewer-probe-2026-09-27.md).

User scope correction, 2026-09-27: human and agent input may occur concurrently.
The human is responsible for conflicts. Do not add input leases, takeover
handshakes, arbitration, agent pausing or cancellation. The detailed delivery
sequence is in the [implementation plan](SiloUI-DETACHED-DESKTOP-IMPLEMENTATION-PLAN.md).

## Decision

**Use the same streamed desktop viewer on every device, for local and remote
computers. The desktop belongs to the computer; the viewer attaches to it.**

The target stack is:

- **Guest session:** Xfce on an independently supervised X11 virtual display,
  initially Xvfb, with its own audio and desktop processes. Use the same guest
  recipe on ARM64 and x86-64. The computer's desktop service owns this session.
- **Capture and streaming:** Selkies 2.0, installed as a native package and
  attached to that existing display/audio session. Supervise it separately.
- **Host viewer:** one shared client based on Selkies' maintained web client,
  embedded in Silo's existing Tauri webview on macOS and Linux. Keep the
  guest-facing webview restricted. Do not add Electron or bundled Chromium.
- **Connection:** Selkies WebSockets through authenticated local forwarding;
  for a remote computer, carry the same endpoint through authenticated SSH.
  Local viewing needs no internet or hosted relay. Both locations use the same
  protocol and user interface.

The viewer owns presentation, playback, and authorized interaction.
It does not own the computer, compositor, desktop applications, or streaming-service
lifetime. Closing or crashing the view must not trigger their shutdown. This
is a lifecycle boundary, not a requirement for another application runtime.
Silo's explicit Quit/computer-stop policy is a separate operation.

The earlier draft introduced Electron to standardize the browser engine.
That recommendation lacked a reproduced Tauri compatibility failure and a
resource comparison. A consistent product requires the same behavior and
protocol, not the same browser engine. This revision removes that dependency.

Do not use `selkies-session` or an all-in-one desktop container as the product
lifecycle owner: those combine session creation with the streaming launch.
Selkies' [native guide](https://github.com/selkies-project/selkies/blob/2.0.0/docs/native.md)
explicitly supports attaching to an existing X11 display/audio service and
separate management when the desktop must outlive the streamer. That is the
integration boundary this design selects.

## The same path on each platform

| Supported device | Local computer | Remote computer |
| --- | --- | --- |
| Apple Silicon macOS 14+ | Tauri/WKWebView; local authenticated forwarding to guest Selkies | Same viewer and stream through SSH |
| Linux x86-64, Ubuntu 24.04-compatible | Tauri/WebKitGTK; local authenticated forwarding to guest Selkies | Same viewer and stream through SSH |
| Linux ARM64, Ubuntu 24.04-compatible | Tauri/WebKitGTK; local authenticated forwarding to guest Selkies | Same viewer and stream through SSH |

The device's architecture selects its viewer binary. The guest's architecture
selects its packages. The network stream does not require the device and remote
guest to share an architecture. A Wayland Linux device can view an X11 guest;
these are separate display systems.

[Tauri documents](https://v2.tauri.app/reference/webview-versions/) WKWebView on
macOS and WebKitGTK on Linux. Silo already embeds its current desktop client
with a Tauri child webview in
[`desktop_viewer.rs`](../app/SiloUI/src-tauri/src/desktop_viewer.rs).
This proves an existing embedding boundary, not Selkies compatibility.
Qualify Selkies' exact codec/decoder path, audio, input and reconnect behavior
on minimum and current supported device environments. A browser API appearing
in documentation does not establish codec availability or hardware decoding.

Selkies publishes architecture-matched native guest packages. Package
availability is not successful Silo qualification on every combination.

## Computer use bypasses the human viewer

```text
Inside each computer:
  Agent backend -> LCU -> the guest's Linux desktop and applications
                            |
                            +-> Selkies -> Tauri viewer -> human observer

Optional human control:
  Tauri viewer -> authorized Selkies input -> that same guest desktop
```

LCU reads the guest's accessibility state/screenshots and operates its desktop
directly. It does not inspect or click Silo's viewer. Install and launch it
inside the computer under the desktop account, attached to the same X11 display and
D-Bus session. Its session launcher discovers an existing Xfce session; direct
mode needs `DISPLAY`, `DBUS_SESSION_BUS_ADDRESS` and any required `XAUTHORITY`.
It does not start the desktop. See the pinned
[LCU installation contract](https://github.com/0xpolarzero/lcu/blob/v0.4.0/docs/INSTALLATION.md).

LCU 0.4.0 requires an existing official desktop app inside the Linux guest and
uses its Node/CUA runtime directly, without launching its Electron UI. Its
guest installation and maintained harness adapters differ from the old LCU
prototype. The implementation plan treats those prerequisites separately from
the Tauri viewer and the currently shipped Luda recipe.

The model can be remote; the desktop tool execution belongs inside the computer.
An ordinary host-side SSH command does not automatically load guest MCP tools.
Opening, closing, resizing or reconnecting the human view must not be a
prerequisite for an agent action. The streamer shows the resulting application
state, including semantic actions that do not visibly move the mouse. Guest
screenshots use guest coordinates, independent of viewer scaling.

The [earlier LCU prototype](research/e2b-lcu-qualification-2026-09-22.md)
recorded direct guest accessibility editing and a separate native viewer.
That evidence is limited to its tested fixture and architecture. The current
production recipe at the time installed [Luda](SiloUI-LUDA.md), since removed; it also operates inside the
guest. This proposal does not silently replace that integration or claim the
new Selkies combination has already passed.

The session remains a VM: application accessibility, virtual graphics and
Linux permissions still determine what an agent can do. Viewer independence
does not promise bare-metal graphics performance. Human input and LCU input
share that session without Silo coordinating them. The viewer keeps direct
human input; connecting and observing sends no unsolicited desktop changes.

## The viewer contract

"Does not impact anything" means **no implicit changes to the desktop's
state or lifetime**. Reading and encoding frames still consumes resources,
and deliberate control naturally changes applications. Measure that overhead;
do not promise zero CPU or zero timing impact on a running task.

| Action or situation | Required behavior |
| --- | --- |
| Open and watch | Attach to the existing session. No unsolicited guest input, clipboard writes, keymap/DPI changes, app launches or display reconfiguration |
| Desktop stopped | Show that state. Starting the desktop is a separate explicit operation, never a side effect of opening a viewer |
| Resize, zoom or fullscreen the viewer | Change the local presentation only. Keep guest resolution, scale and monitor topology stable |
| Change guest resolution | Explicit controller operation in Display settings; persist it with the session |
| Close every viewer | Keep the desktop, apps and agent running; stop unused capture/encoding when safe |
| Viewer crash, reload or network loss | Reattach to the same session; reject stale/replayed input and re-read geometry |
| Streaming-service restart | Keep display, audio session and apps running, then reattach capture |
| Observe an agent | Stream the same session the agent operates; observation neither claims control nor pauses the agent |
| Type or click | Forward intentional human input without pausing, revoking or coordinating the agent; concurrent input is the human's responsibility |
| Additional viewing connections | No competing resize or clipboard synchronization; a separate public read-only sharing product is outside this plan |
| Stop desktop or computer | Use the explicit lifecycle command and show its consequences; this is not a viewer disconnect |

An explicitly delegated agent task retains authorization until revoked or
completed; do not insert repeated permission prompts. Supported agents operate
in the guest even when no human viewer is open. Keep the existing direct human
input and authenticated native gateway. No new interaction toggle, observer
credential system, input ownership or agent-input adapter is part of this work.
Release only keys/buttons held by a disconnecting viewer where supported; test
recovery without global input resets.

Selkies' [settings documentation](https://github.com/selkies-project/selkies/blob/2.0.0/docs/settings.md)
enables automatic resize and clipboard functionality by default. Disable
automatic resize and unsolicited clipboard synchronization, and qualify
intentional copy/paste in the native webviews. Connection-time DPI/keymap state,
display attachment and last-viewer teardown require state-diff and
process-survival tests. Upstream viewer roles do not prove those properties.
Repair reusable gaps upstream before shipping.

## Coverage of the actual workflows

| Workflow | One-stack behavior |
| --- | --- |
| Coding, documents and static UI | Negotiate an available text-preserving encoding; restore fine detail after motion; keep scaling local |
| Browser animation and video | Adapt frame rate/encoding within Selkies; preserve input responsiveness |
| CPU-only guest | Mandatory supported path with software rendering/encoding; no guest GPU prerequisite |
| Guest with a supported encoder | Use hardware encoding within the same service when detected and qualified; host GPU presence alone is insufficient |
| Clipboard and files | Deliberate controller operations with direction and completion feedback; observing does not overwrite either clipboard. Text and images (1 MiB and 16 MiB limits) move through Paste into computer and Copy from computer in the toolbar, with Command+V/C on macOS and Ctrl+Shift+V/C on Linux; see [SiloUI-DESKTOP.md](SiloUI-DESKTOP.md#clipboard-behaviour). Files are separate |
| Audio | Playback from the same session; microphone/camera require a separate activation |
| Multiple views or displays | Reuse the current per-computer viewer; no automatic guest display creation/removal on window open or close |
| Computer checkpoints | Reconnect after the computer's supported restore operation; independently verify guest devices and process state. Viewer reconnect is not a checkpoint |

"All use cases" here means Silo's supported Linux GUI, human/agent, local/remote
and device-platform workflows. It does not assert that every guest desktop,
3D application, HDR mode, USB peripheral, IME or codec is already qualified.
Encoding support also does not supply a GPU to applications inside a VM.

## Deliberate tradeoffs

Choose one tested delivery path over a separate Apple display API, QEMU/SPICE
viewer, RDP client and gaming client. This pays local encoding overhead in
exchange for one session contract, one client implementation and one set of interaction
semantics. If this single route cannot meet the local latency/resource budget,
the decision must be revisited with measurements. Do not disguise that failure
by silently shipping different feature sets on different devices.

Keeping Tauri avoids adding a second application runtime. An open webview,
decoder and frame buffers still consume host memory; capture and encoding
consume guest resources. No comparative memory measurements have been made.
Measure total host and guest CPU/memory with no view, one view and two views,
including stream startup, motion and closed-view resource release. Suspend
unused capture/encoding without stopping the desktop. Test this behavior;
do not assume disconnecting a client automatically implements it.

Xfce/X11 is selected because it supplies a full desktop through the documented
independent-display attachment route on both guest architectures. Appearance,
fonts, scaling and application defaults can be improved without coupling
session lifetime to a new compositor. The decision is not a claim that Xfce is
the fastest desktop or that GNOME/Wayland is inferior. A Wayland migration must
later preserve this same viewer contract; it is not a second shipping path.

WebSockets over the existing authenticated connection supplies one reachable
baseline, including restrictive networks. TCP loss can delay frames. If WAN
tests demand it, qualify Selkies' own WebRTC mode as a transport enhancement
inside the same engine, preserving the same UI and permissions; do not make
users select a different desktop product. No TURN infrastructure is required
for the selected initial WebSocket route.

## Release and trust boundaries

Pin Selkies, the guest display/desktop/audio packages and application
dependencies. Record the device webview/codec versions in qualification results;
system webview updates are outside the application's package pin. Record
component updates, reproducible artifacts and rollback.
Selkies 2.0 is newly released; the existing Kasm implementation remains the
production control until this one replacement passes the platform matrix.
That is a migration boundary, not a proposal for a permanent protocol chooser.

Selkies uses MPL-covered components and codec build variants with additional
obligations. Review the actual distribution manifest. Keep guest content
outside privileged host capabilities, verify session authorization, and expose
only the specific host bridges needed by the current mode. Apply
[Tauri's capability boundaries](https://v2.tauri.app/security/capabilities/)
to the viewer. Do not expose unauthenticated guest display ports publicly.

## One decisive prototype

Create one disposable guest with an unsaved editor buffer, a running GUI app,
an audio session and a supported agent. Attach a viewer and exercise human input,
close the viewer, crash/restart Selkies, interrupt the connection, and attach
again. Verify the same guest session/process identities, unchanged desktop
geometry and no unsolicited input or clipboard writes. Repeat without any
human input so connection-time side effects cannot pass unnoticed. A second
passive viewing connection is a diagnostic case, not a new sharing feature.

Run an LCU task before any viewer has connected, while a viewer watches without
sending input, after every viewer closes, and through a streamer restart. Verify
the same independently checked saved result in each case. Check that observer
window resizing never changes the guest geometry used by LCU. Viewing or
using human input must never pause or reconfigure the agent.

Run this exact artifact on the three supported device builds, locally and over
SSH. Apply the implementation plan's behavior and resource gates. Verify
keyboard layouts/IME and codec negotiation in the actual
WKWebView/WebKitGTK environments; passing in an external browser is not proof
of embedded-viewer compatibility.

**Next action:** qualify this single detached-viewer prototype across the
supported matrix. If it fails the lifecycle contract, fix the boundary before
polishing the viewer or changing desktop environments.
